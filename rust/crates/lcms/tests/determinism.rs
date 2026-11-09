//! determinism manifest
#![allow(unsafe_op_in_unsafe_fn)]
//!
//! `cargo test -p lcms --test determinism -- --nocapture` prints one
//! `CELL <name>|<rust-hash>|<c-hash>|<eq>|<maxdiff>` line per cell plus
//! a `SUMMARY|<r>|<c>` digest. Run the same binary on every platform
//! and diff the manifests: the C column shows the oracle's own drift,
//! the Rust column the port's. All profiles are built in-memory so
//! the test is self-contained.

use lcms::{cam02, gamma, pcs, profile, types::*, virt, wtpnt, xform};
use lcms::profile::IccProfile;
use lcms_cref as c;
use std::ffi::c_void;

fn fnv(b: &[u8]) -> u64 {
    // zenhash::xxh3_64 — bit-identical on every target/SIMD tier, so the
    // per-cell hashes are meaningful to compare across CI/platforms.
    zenhash::xxh3_64(b)
}

thread_local! {
    static CELLS: std::cell::RefCell<Vec<(String, Vec<u8>, Vec<u8>)>> =
        std::cell::RefCell::new(Vec::new());
}

/// manifest-level digest: xxh3 over every cell's bytes in order — one
/// value CI can diff across platforms.
fn digest(cells: &[(String, Vec<u8>, Vec<u8>)]) -> u64 {
    let mut h = zenhash::Xxh3::new();
    for (name, r, cb) in cells {
        h.update(name.as_bytes());
        h.update(r);
        h.update(cb);
    }
    h.digest()
}

/// zero the volatile header fields (creation datetime, profile ID)
fn mask(b: &mut Vec<u8>) {
    for i in 0..12 { b.get_mut(24 + i).map(|x| *x = 0); }
    for i in 0..16 { b.get_mut(84 + i).map(|x| *x = 0); }
}

fn emit(name: &str, r: &[u8], cb: &[u8]) {
    CELLS.with(|c| c.borrow_mut().push(
        (name.to_string(), r.to_vec(), cb.to_vec())));
    let (rh, ch) = (fnv(r), fnv(cb));
    let eq = r == cb;
    // element-wise diffs: f32-ulp for xfl cells, u16 otherwise
    let mut dmax = 0u64;
    if !eq {
        if name.starts_with("xfl:") {
            let rf: Vec<f32> = r.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0],b[1],b[2],b[3]])).collect();
            let cf: Vec<f32> = cb.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0],b[1],b[2],b[3]])).collect();
            let mut mx = 0u64;
            for (a, b) in rf.iter().zip(cf.iter()) {
                let da = (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs();
                let rel = (a - b).abs();
                mx = mx.max(da.min(1u64 << 62));
                if rel > 1e-4 { mx = mx.max(1u64 << 62); }
            }
            dmax = mx; // f32 ulp diff, capped at huge when >1e-4 abs
        } else if r.len() % 2 == 0 {
            dmax = r.chunks_exact(2).zip(cb.chunks_exact(2))
                .map(|(x, y)| {
                    let a = u16::from_le_bytes([x[0], x[1]]) as i64;
                    let b = u16::from_le_bytes([y[0], y[1]]) as i64;
                    (a - b).unsigned_abs()
                }).max().unwrap_or(0);
        }
    }
    println!("CELL {name}|{rh:016x}|{ch:016x}|{}|{dmax}", eq as u8);
}

fn emit_f64(name: &str, r: f64, cv: f64) {
    emit(name, &r.to_bits().to_le_bytes(), &cv.to_bits().to_le_bytes());
}

// ---- sweeps ----------------------------------------------------

/// per-channel gradient ramps + weyl-sequence noise + edge patterns
fn sweep16(ch: usize) -> Vec<u16> {
    let mut v = Vec::new();
    for cc in 0..ch {
        for lvl in [0u16, 21845, 43690, 65535] {
            for i in 0..=255u16 {
                let mut px = vec![lvl; ch];
                px[cc] = i * 257;
                v.extend_from_slice(&px);
            }
        }
    }
    for i in 0..8192u32 {
        for cc in 0..ch {
            let w = i.wrapping_mul(2654435761)
                    .wrapping_add(cc as u32 * 0x9e37)
                    .wrapping_add((i % 7) * 8192);
            v.push((w >> 8) as u16);
        }
    }
    let npat = 1usize << ch.min(10);
    for pat in 0..npat {
        for _ in 0..2 {
            for cc in 0..ch {
                v.push(if pat >> cc & 1 == 1 { 0xFFFF } else { 0 });
            }
        }
    }
    v
}

fn sweepf(ch: usize) -> Vec<f32> {
    let mut v: Vec<f32> = sweep16(ch).iter()
        .map(|&u| u as f32 / 65535.0).collect();
    // explicit out-of-domain + edge doubles
    for &e in &[0.0f32, -0.0, 1.0, 1.5, -0.5, 1e-30, 1e30,
                0.04045, 0.0031308, 0.5] {
        for cc in 0..ch {
            let mut px = vec![e; ch];
            px[cc] = if cc == 0 { e } else { 1.0 - e };
            v.extend_from_slice(&px);
        }
    }
    v
}

fn f16(pt: u32, ch: u32) -> u32 { pt << 16 | ch << 3 | 2 }
fn ff32(pt: u32, ch: u32) -> u32 { 1 << 22 | pt << 16 | ch << 3 | 4 }

// ---- canonical profiles (C-built bytes) ------------------------

unsafe fn save_c(p: *mut c_void) -> Vec<u8> {
    // cmsSaveProfileToMem's *BytesNeeded is IN-only on write — the
    // real size comes from the NULL query, not the post-write value.
    let mut n0: u32 = 0;
    if c::cmsSaveProfileToMem(p, std::ptr::null_mut(), &mut n0) == 0 || n0 == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; n0 as usize];
    let mut n = buf.len() as u32;
    if c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut n) == 0 {
        return Vec::new();
    }
    buf.truncate(n0 as usize);
    buf
}

unsafe fn c_srgb_prim() -> c::CiexyYTriple {
    c::CiexyYTriple {
        Red: c::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
        Green: c::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
        Blue: c::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
    }
}

unsafe fn c_param4() -> *mut c::ToneCurve {
    let prm = [2.4, 1.0 / 1.055, 0.055 / 1.055,
               1.0 / 12.92, 0.04045, 0.0, 0.0, 0.0, 0.0, 0.0];
    c::cmsBuildParametricToneCurve(std::ptr::null_mut(), 4, prm.as_ptr())
}

/// mpet wrapper: cvst + clutf element for in_ch -> out_ch
unsafe fn c_mpet_pipeline(in_ch: u32, out_ch: u32) -> *mut c_void {
    let lut = c::cmsPipelineAlloc(std::ptr::null_mut(), in_ch, out_ch);
    let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
    let cv = c::cmsStageAllocToneCurves(std::ptr::null_mut(), in_ch,
        vec![g; in_ch as usize].as_ptr());
    c::cmsPipelineInsertStage(lut, 1, cv);
    let npts = 6u32;
    let total = (npts as usize).pow(in_ch) * out_ch as usize;
    let mut tab = vec![0.0f32; total];
    for (i, x) in tab.iter_mut().enumerate() {
        *x = (i % 4096) as f32 / 4096.0;
    }
    let cl = c::cmsStageAllocCLutFloat(std::ptr::null_mut(), npts,
        in_ch, out_ch, tab.as_ptr());
    c::cmsPipelineInsertStage(lut, 1, cl);
    lut
}

/// mAB pipeline: cvst + clut16 + cvst (shape 3 — a lone matf/clutf
/// is NOT a valid LutAToB shape, and float CLUTs are rejected by
/// WriteCLUT: "CLUT are 8 or 16 bit only")
unsafe fn c_mab_pipeline(in_ch: u32, out_ch: u32) -> *mut c_void {
    let lut = c::cmsPipelineAlloc(std::ptr::null_mut(), in_ch, out_ch);
    let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.7);
    let b = c::cmsStageAllocToneCurves(std::ptr::null_mut(), in_ch,
        vec![g; in_ch as usize].as_ptr());
    c::cmsPipelineInsertStage(lut, 0, b);
    let npts = 9u32;
    let total = (npts as usize).pow(in_ch) * out_ch as usize;
    let mut tab = vec![0u16; total];
    for (i, x) in tab.iter_mut().enumerate() {
        *x = ((i * 97) % 65536) as u16;
    }
    let cl = c::cmsStageAllocCLut16bit(std::ptr::null_mut(), npts,
        in_ch, out_ch, tab.as_ptr());
    c::cmsPipelineInsertStage(lut, 1, cl);
    let a = c::cmsStageAllocToneCurves(std::ptr::null_mut(), out_ch,
        vec![g; out_ch as usize].as_ptr());
    c::cmsPipelineInsertStage(lut, 1, a);
    lut
}

/// mft2 pipeline: cvst + clut16
unsafe fn c_mft2_pipeline(in_ch: u32, out_ch: u32) -> *mut c_void {
    let lut = c::cmsPipelineAlloc(std::ptr::null_mut(), in_ch, out_ch);
    let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.7);
    let cv = c::cmsStageAllocToneCurves(std::ptr::null_mut(), in_ch,
        vec![g; in_ch as usize].as_ptr());
    c::cmsPipelineInsertStage(lut, 1, cv);
    let npts = 9u32;
    let total = (npts as usize).pow(in_ch) * out_ch as usize;
    let mut tab = vec![0u16; total];
    for (i, x) in tab.iter_mut().enumerate() {
        *x = ((i * 97) % 65536) as u16;
    }
    let cl = c::cmsStageAllocCLut16bit(std::ptr::null_mut(), npts,
        in_ch, out_ch, tab.as_ptr());
    c::cmsPipelineInsertStage(lut, 1, cl);
    lut
}


unsafe fn c_mandatory_tags(p: *mut c_void) {
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu); // 'desc'
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2); // 'cprt'
    let mlu3 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x77747074, mlu3); // 'wtpt' as mluc? no — wtpt is XYZ
    c::cmsMLUfree(mlu3); // don't leave wrong-type tag
    c::cmsMLUfree(mlu);
}

unsafe fn c_profiles() -> Vec<(String, Vec<u8>)> {
    let ctx = std::ptr::null_mut();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut push = |name: &str, p: *mut c_void| {
        if !p.is_null() {
            out.push((name.to_string(), save_c(p)));
            c::cmsCloseProfile(p);
        }
    };
    push("srgb", c::cmsCreate_sRGBProfile());
    let wt = *c::cmsD50_xyY();
    let prim = c_srgb_prim();
    let trc = c_param4();
    let t3 = [trc, trc, trc];
    push("rgbp4", c::cmsCreateRGBProfile(
        &wt as *const _ as *const _,
        &prim as *const _ as *const _, t3.as_ptr() as *const *mut c_void));
    let g22 = c::cmsBuildGamma(ctx, 2.2);
    let g3 = [g22, g22, g22];
    push("rgb22", c::cmsCreateRGBProfile(
        &wt as *const _ as *const _,
        &prim as *const _ as *const _, g3.as_ptr() as *const *mut c_void));
    push("gray", c::cmsCreateGrayProfile(
        &wt as *const _ as *const _, g22));
    push("lab2", c::cmsCreateLab2Profile(std::ptr::null()));
    push("lab4", c::cmsCreateLab4Profile(std::ptr::null()));
    // cmyk mft2: placeholder + A2B0 + B2A0
    let p = c::cmsCreateProfilePlaceholder(ctx);
    c::cmsSetDeviceClass(p, 0x70727472); // 'prtr'
    c::cmsSetColorSpace(p, 0x434D594B);  // 'CMYK'
    c::cmsSetPCS(p, 0x4C616220);         // 'Lab '
    c::cmsSetProfileVersion(p, 2.1);
    c::cmsWriteTag(p, 0x41324230, c_mft2_pipeline(4, 3));
    c::cmsWriteTag(p, 0x42324130, c_mft2_pipeline(3, 4));
    let wt2 = *c::cmsD50_XYZ();
    c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu);
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2);
    let chad: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
    c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
    push("cmyk", p);
    // mAB: mntr RGB + A2B0 pipeline (cvst+clutf)
    let p = c::cmsCreateProfilePlaceholder(ctx);
    c::cmsSetDeviceClass(p, 0x6D6E7472); // 'mntr'
    c::cmsSetColorSpace(p, 0x52474220);
    c::cmsSetPCS(p, 0x4C616220);
    c::cmsSetProfileVersion(p, 4.4);
    c::cmsWriteTag(p, 0x41324230, c_mab_pipeline(3, 3));
    let wt2 = *c::cmsD50_XYZ();
    c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu);
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2);
    let chad: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
    c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
    push("mab", p);
    // mpetin: 'mntr' cmyk + DToB0 mpet 4->3 (float input path)
    let p = c::cmsCreateProfilePlaceholder(ctx);
    c::cmsSetDeviceClass(p, 0x6D6E7472);
    c::cmsSetColorSpace(p, 0x434D594B);
    c::cmsSetPCS(p, 0x4C616220);
    c::cmsSetProfileVersion(p, 4.4);
    c::cmsWriteTag(p, 0x64326230, c_mpet_pipeline(4, 3)); // 'd2b0'
    let wt2 = *c::cmsD50_XYZ();
    c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu);
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2);
    let chad: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
    c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
    push("mpetin", p);
    // mpetout: 'mntr' cmyk + BToD0 mpet 3->4 (float output path)
    let p = c::cmsCreateProfilePlaceholder(ctx);
    c::cmsSetDeviceClass(p, 0x6D6E7472);
    c::cmsSetColorSpace(p, 0x434D594B);
    c::cmsSetPCS(p, 0x4C616220);
    c::cmsSetProfileVersion(p, 4.4);
    c::cmsWriteTag(p, 0x62326430, c_mpet_pipeline(3, 4)); // 'b2d0'
    let wt2 = *c::cmsD50_XYZ();
    c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu);
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2);
    let chad: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
    c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
    push("mpetout", p);
    // labdl: 'link' Lab->Lab A2B0 clut devicelink
    let p = c::cmsCreateProfilePlaceholder(ctx);
    c::cmsSetDeviceClass(p, 0x6C696E6B); // 'link'
    c::cmsSetColorSpace(p, 0x4C616220);
    c::cmsSetPCS(p, 0x4C616220);
    c::cmsSetProfileVersion(p, 2.1);
    c::cmsWriteTag(p, 0x41324230, c_mft2_pipeline(3, 3));
    let wt2 = *c::cmsD50_XYZ();
    c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
    let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
    c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
    c::cmsWriteTag(p, 0x64657363, mlu);
    let mlu2 = c::cmsMLUdup(mlu);
    c::cmsWriteTag(p, 0x63707274, mlu2);
    let chad: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
    c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
    push("labdl", p);
    // cicp: rgb profile + 'cicp' 4-byte tag (H.273: BT709/sRGB/identity/FR)
    let p = c::cmsCreateRGBProfile(
        &wt as *const _ as *const _,
        &prim as *const _ as *const _, t3.as_ptr() as *const *mut c_void);
    let cicp = [1u8, 13, 0, 1];
    c::cmsWriteRawTag(p, 0x63696370, cicp.as_ptr() as *const _, 4);
    push("cicp", p);
    out
}

// ---- transforms ------------------------------------------------

unsafe fn run_x16(in_b: &[u8], out_b: &[u8],
                  in_fmt: u32, out_fmt: u32, ch_in: usize,
                  intent: u32) -> (Vec<u8>, Vec<u8>) {
    let ip = c::cmsOpenProfileFromMem(in_b.as_ptr() as *const _, in_b.len() as u32);
    let op = if out_b.is_empty() { std::ptr::null_mut() }
             else { c::cmsOpenProfileFromMem(out_b.as_ptr() as *const _, out_b.len() as u32) };
    let t = if op.is_null() {
        let mut arr = [ip];
        c::cmsCreateMultiprofileTransform(arr.as_mut_ptr(), 1, in_fmt, out_fmt, intent, 0)
    } else {
        c::cmsCreateTransform(ip, in_fmt, op, out_fmt, intent, 0)
    };
    let sw = sweep16(ch_in);
    let np = sw.len() / ch_in;
    let och = ((out_fmt >> 3) & 0xF) as usize;
    let mut cb = vec![0u8; np * och * 2];
    let mut rb = vec![0u8; np * och * 2];
    if !t.is_null() {
        c::cmsDoTransform(t, sw.as_ptr() as *const _,
            cb.as_mut_ptr() as *mut _, np as u32);
        c::cmsDeleteTransform(t);
    }
    if !ip.is_null() { c::cmsCloseProfile(ip); }
    if !op.is_null() { c::cmsCloseProfile(op); }
    // rust side
    let Some(rip) = profile::open_profile_from_mem(None, in_b)
        else { return (Vec::new(), cb); };
    let rop = if out_b.is_empty() { None }
                  else { profile::open_profile_from_mem(None, out_b) };
    {
        let mut bpc = vec![false];
        let intents = vec![intent];
        let adapt = vec![1.0f64];
        let mut profs: Vec<Box<IccProfile>> = vec![rip];
        let mut rt = match rop {
            Some(o) => {
                profs.push(o);
                let mut bpc = vec![false; 2];
                let intents = vec![intent; 2];
                let adapt = vec![1.0; 2];
                xform::create_extended_transform(None, &mut profs,
                    &mut bpc, &intents, &adapt, None, 0,
                    in_fmt, out_fmt, 0)
            }
            None => xform::create_extended_transform(None,
                &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0),
        };
        if let Some(t) = rt.as_mut() {
            t.do_transform(byt2(&sw), &mut rb, np as u32);
        }
    }
    (rb, cb)
}

fn byt2(v: &[u16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(
        v.as_ptr() as *const u8, v.len() * 2) }
}

unsafe fn run_xfl(in_b: &[u8], out_b: &[u8],
                  in_fmt: u32, out_fmt: u32, ch_in: usize,
                  intent: u32) -> (Vec<u8>, Vec<u8>) {
    let ip = c::cmsOpenProfileFromMem(in_b.as_ptr() as *const _, in_b.len() as u32);
    let op = if out_b.is_empty() { std::ptr::null_mut() }
             else { c::cmsOpenProfileFromMem(out_b.as_ptr() as *const _, out_b.len() as u32) };
    let t = if op.is_null() {
        let mut arr = [ip];
        c::cmsCreateMultiprofileTransform(arr.as_mut_ptr(), 1, in_fmt, out_fmt, intent, 0)
    } else {
        c::cmsCreateTransform(ip, in_fmt, op, out_fmt, intent, 0)
    };
    let sw = sweepf(ch_in);
    let np = sw.len() / ch_in;
    let och = ((out_fmt >> 3) & 0xF) as usize;
    let mut cb = vec![0u8; np * och * 4];
    let mut rb = vec![0u8; np * och * 4];
    if !t.is_null() {
        c::cmsDoTransform(t, sw.as_ptr() as *const _,
            cb.as_mut_ptr() as *mut _, np as u32);
        c::cmsDeleteTransform(t);
    }
    if !ip.is_null() { c::cmsCloseProfile(ip); }
    if !op.is_null() { c::cmsCloseProfile(op); }
    let Some(rip) = profile::open_profile_from_mem(None, in_b)
        else { return (Vec::new(), cb); };
    let rop = if out_b.is_empty() { None }
              else { profile::open_profile_from_mem(None, out_b) };
    {
        let mut bpc = vec![false];
        let intents = vec![intent];
        let adapt = vec![1.0f64];
        let mut profs: Vec<Box<IccProfile>> = vec![rip];
        let mut rt = match rop {
            Some(o) => {
                profs.push(o);
                let mut bpc = vec![false; 2];
                let intents = vec![intent; 2];
                let adapt = vec![1.0; 2];
                xform::create_extended_transform(None, &mut profs,
                    &mut bpc, &intents, &adapt, None, 0,
                    in_fmt, out_fmt, 0)
            }
            None => xform::create_extended_transform(None,
                &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0),
        };
        if let Some(t) = rt.as_mut() {
            let ib = unsafe { std::slice::from_raw_parts(
                sw.as_ptr() as *const u8, sw.len() * 4) };
            t.do_transform(ib, &mut rb, np as u32);
        }
    }
    (rb, cb)
}

// ---- scalar cells ----------------------------------------------

unsafe fn scalars() {
    let d50 = wtpnt::d50_xyz();
    let d50c = *c::cmsD50_XYZ();
    // Lab<->XYZ + Lab<->LCh over a grid
    let mut rb = Vec::new(); let mut cb = Vec::new();
    for l in 0..=16u32 {
        for a in 0..=16u32 {
            for b in 0..=16u32 {
                let lab = CieLab {
                    L: l as f64 * 6.25,
                    a: a as f64 * 16.0 - 128.0,
                    b: b as f64 * 16.0 - 128.0 };
                let xyz = pcs::lab2xyz(Some(&d50), &lab);
                rb.extend_from_slice(&xyz.X.to_bits().to_le_bytes());
                rb.extend_from_slice(&xyz.Y.to_bits().to_le_bytes());
                rb.extend_from_slice(&xyz.Z.to_bits().to_le_bytes());
                let lab2 = pcs::xyz2lab(Some(&d50), &xyz);
                rb.extend_from_slice(&lab2.L.to_bits().to_le_bytes());
                let lch = pcs::lab2lch(&lab);
                rb.extend_from_slice(&lch.h.to_bits().to_le_bytes());
                let l2 = pcs::lch2lab(&lch);
                rb.extend_from_slice(&l2.a.to_bits().to_le_bytes());
                // C side
                let clab = c::CieLab { L: lab.L, a: lab.a, b: lab.b };
                let mut cxyz = c::CieXyz::default();
                c::cmsLab2XYZ(&d50c, &mut cxyz, &clab);
                cb.extend_from_slice(&cxyz.X.to_bits().to_le_bytes());
                cb.extend_from_slice(&cxyz.Y.to_bits().to_le_bytes());
                cb.extend_from_slice(&cxyz.Z.to_bits().to_le_bytes());
                let mut clab2 = c::CieLab::default();
                c::cmsXYZ2Lab(&d50c, &mut clab2, &cxyz);
                cb.extend_from_slice(&clab2.L.to_bits().to_le_bytes());
                let mut clch = c::CieLCh::default();
                c::cmsLab2LCh(&mut clch, &clab);
                cb.extend_from_slice(&clch.h.to_bits().to_le_bytes());
                let mut cl2 = c::CieLab::default();
                c::cmsLCh2Lab(&mut cl2, &clch);
                cb.extend_from_slice(&cl2.a.to_bits().to_le_bytes());
            }
        }
    }
    emit("pcs", &rb, &cb);
    // delta-E family
    let mut rb = Vec::new(); let mut cb = Vec::new();
    for i in 0..512u32 {
        let l1 = CieLab { L: (i % 100) as f64,
            a: (i as f64 - 255.0) / 4.0,
            b: ((i * 7) as f64 - 160.0) / 3.0 };
        let l2 = CieLab { L: ((i * 3) % 100) as f64,
            a: ((i * 5) as f64 - 255.0) / 5.0,
            b: ((i * 11) as f64 - 160.0) / 7.0 };
        let cl1 = c::CieLab { L: l1.L, a: l1.a, b: l1.b };
        let cl2 = c::CieLab { L: l2.L, a: l2.a, b: l2.b };
        rb.extend_from_slice(&pcs::delta_e(&l1, &l2).to_bits().to_le_bytes());
        rb.extend_from_slice(&pcs::cie94_delta_e(&l1, &l2).to_bits().to_le_bytes());
        rb.extend_from_slice(&pcs::bfd_delta_e(&l1, &l2).to_bits().to_le_bytes());
        rb.extend_from_slice(&pcs::cie2000_delta_e(&l1, &l2, 1.0, 1.0, 1.0).to_bits().to_le_bytes());
        cb.extend_from_slice(&c::cmsDeltaE(&cl1, &cl2).to_bits().to_le_bytes());
        cb.extend_from_slice(&c::cmsCIE94DeltaE(&cl1, &cl2).to_bits().to_le_bytes());
        cb.extend_from_slice(&c::cmsBFDdeltaE(&cl1, &cl2).to_bits().to_le_bytes());
        cb.extend_from_slice(&c::cmsCIE2000DeltaE(&cl1, &cl2, 1.0, 1.0, 1.0).to_bits().to_le_bytes());
    }
    emit("deltaE", &rb, &cb);
    // parametric curves types 1-8: eval16 + eval_float + estimate
    let mut rb = Vec::new(); let mut cb = Vec::new();
    for ty in 1..=8i32 {
        let params = [2.4, 1.0, 0.0, 1.0, 0.0, 0.04045, 0.0, 0.0, 0.0, 0.0];
        let rg = gamma::build_parametric_tone_curve(None, ty, &params);
        let cg = c::cmsBuildParametricToneCurve(
            std::ptr::null_mut(), ty, params.as_ptr());
        if let (Some(rv), cv) = (rg, cg) {
            if !cv.is_null() {
                for i in 0..4096u32 {
                    let u = (i * 16).min(65535) as u16;
                    rb.extend_from_slice(&rv.eval16(u).to_le_bytes());
                    cb.extend_from_slice(&c::cmsEvalToneCurve16(cv, u).to_le_bytes());
                    let f = i as f32 / 4095.0;
                    rb.extend_from_slice(&rv.eval_float(f).to_bits().to_le_bytes());
                    cb.extend_from_slice(&c::cmsEvalToneCurveFloat(cv, f).to_bits().to_le_bytes());
                }
                emit_f64(&format!("estg{ty}"),
                    gamma::estimate_gamma(&rv, 0.001),
                    c::cmsEstimateGamma(cv, 0.001));
                c::cmsFreeToneCurve(cv);
            }
        }
    }
    emit("param", &rb, &cb);
    // CIECAM02 fwd/rev
    let rvc = ViewingConditions {
        white_point: wtpnt::d50_xyz(),
        Yb: 20.0, La: 40.0, surround: 2, D_value: 1.0 };
    let cvc = c::ViewingConditions {
        white_point: *c::cmsD50_XYZ(),
        yb: 20.0, la: 40.0, surround: 2, d_value: 1.0 };
    let rcam = cam02::CieCam02::new(None, &rvc);
    let ccam = c::cmsCIECAM02Init(std::ptr::null_mut(), &cvc);
    let mut rb = Vec::new(); let mut cb = Vec::new();
    for i in 0..1024u32 {
        let xyz = CieXyz {
            X: (i % 100) as f64,
            Y: ((i * 3) % 100) as f64,
            Z: ((i * 7) % 100) as f64 };
        let mut jch = JCh::default();
        rcam.forward(&xyz, &mut jch);
        let mut back = CieXyz::default();
        rcam.reverse(&jch, &mut back);
        rb.extend_from_slice(&jch.J.to_bits().to_le_bytes());
        rb.extend_from_slice(&jch.C.to_bits().to_le_bytes());
        rb.extend_from_slice(&jch.h.to_bits().to_le_bytes());
        rb.extend_from_slice(&back.X.to_bits().to_le_bytes());
        let cxyz = c::CieXyz { X: xyz.X, Y: xyz.Y, Z: xyz.Z };
        let mut cjch = c::JCh::default();
        c::cmsCIECAM02Forward(ccam, &cxyz, &mut cjch);
        let mut cback = c::CieXyz::default();
        c::cmsCIECAM02Reverse(ccam, &cjch, &mut cback);
        cb.extend_from_slice(&cjch.j.to_bits().to_le_bytes());
        cb.extend_from_slice(&cjch.c_.to_bits().to_le_bytes());
        cb.extend_from_slice(&cjch.h.to_bits().to_le_bytes());
        cb.extend_from_slice(&cback.X.to_bits().to_le_bytes());
    }
    c::cmsCIECAM02Done(ccam);
    emit("cam02", &rb, &cb);
    // chromatic adaptation
    let mut rb = Vec::new(); let mut cb = Vec::new();
    for i in 0..256u32 {
        let src = CieXyz { X: (i % 90) as f64 + 5.0,
            Y: ((i * 3) % 90) as f64 + 5.0,
            Z: ((i * 7) % 90) as f64 + 5.0 };
        let wp1 = CieXyz { X: 96.42, Y: 100.0, Z: 82.49 };
        let wp2 = CieXyz { X: 95.05, Y: 100.0, Z: 108.88 };
        let out = wtpnt::adapt_to_illuminant(&wp1, &wp2, &src)
            .unwrap_or_default();
        rb.extend_from_slice(&out.X.to_bits().to_le_bytes());
        rb.extend_from_slice(&out.Y.to_bits().to_le_bytes());
        rb.extend_from_slice(&out.Z.to_bits().to_le_bytes());
        let csrc = c::CieXyz { X: src.X, Y: src.Y, Z: src.Z };
        let cwp1 = c::CieXyz { X: 96.42, Y: 100.0, Z: 82.49 };
        let cwp2 = c::CieXyz { X: 95.05, Y: 100.0, Z: 108.88 };
        let mut cout = c::CieXyz::default();
        c::cmsAdaptToIlluminant(&mut cout, &cwp1, &cwp2, &csrc);
        cb.extend_from_slice(&cout.X.to_bits().to_le_bytes());
        cb.extend_from_slice(&cout.Y.to_bits().to_le_bytes());
        cb.extend_from_slice(&cout.Z.to_bits().to_le_bytes());
    }
    emit("adapt", &rb, &cb);
}

#[test]
fn manifest() {
    unsafe {
        let profs = c_profiles();
        // ser cells: canonical bytes -> reopen+resave on each side
        for (name, bytes) in &profs {
            let cp = c::cmsOpenProfileFromMem(
                bytes.as_ptr() as *const _, bytes.len() as u32);
            let mut cbytes = if !cp.is_null() {
                let mut b = save_c(cp);
                c::cmsCloseProfile(cp);
                mask(&mut b); b } else { Vec::new() };
            let mut rbytes = profile::open_profile_from_mem(None, bytes)
                .and_then(|mut icc| profile::save_profile_to_mem(&mut icc))
                .unwrap_or_default();
            mask(&mut cbytes);
            mask(&mut rbytes);
            emit(&format!("ser:{name}"), &rbytes, &cbytes);
        }
        // transform cells
        let get = |n: &str| -> &[u8] {
            &profs.iter().find(|p| p.0 == n).unwrap().1 };
        let plan: &[(&str, &str, u32, u32, u32, u32, &[u32])] = &[
            ("srgb", "lab4", 4, 3, 10, 3, &[0, 1]),
            ("srgb", "gray", 4, 3, 3, 1, &[0]),
            ("rgbp4", "rgb22", 4, 3, 4, 3, &[0]),
            ("lab4", "srgb", 10, 3, 4, 3, &[0, 1]),
            ("cmyk", "srgb", 6, 4, 4, 3, &[0]),
            ("srgb", "cmyk", 4, 3, 6, 4, &[0]),
            ("mab", "srgb", 4, 3, 4, 3, &[0]),
            ("mpetin", "srgb", 6, 4, 4, 3, &[0]),
            ("srgb", "mpetout", 4, 3, 6, 4, &[0]),
            ("cicp", "lab4", 4, 3, 10, 3, &[0]),
        ];
        for (inm, outm, ipt, ich, opt, och, intents) in plan {
            for &i in intents.iter() {
                let (r, cb) = run_x16(get(inm), get(outm),
                    f16(*ipt, *ich), f16(*opt, *och), *ich as usize, i);
                emit(&format!("x16:{inm}>{outm}:{i}"), &r, &cb);
                let (r, cb) = run_xfl(get(inm), get(outm),
                    ff32(*ipt, *ich), ff32(*opt, *och), *ich as usize, i);
                emit(&format!("xfl:{inm}>{outm}:{i}"), &r, &cb);
            }
        }
        // labdl single-profile devicelink
        let (r, cb) = run_x16(get("labdl"), &[],
            f16(10, 3), f16(10, 3), 3, 0);
        emit("x16:labdl", &r, &cb);
        let (r, cb) = run_xfl(get("labdl"), &[],
            ff32(10, 3), ff32(10, 3), 3, 0);
        emit("xfl:labdl", &r, &cb);
        scalars();
        let cells = CELLS.with(|c| c.borrow().clone());
        let d = digest(&cells);
        eprintln!("DIGEST {d:016x} ({} cells)", cells.len());
        // golden baseline: LCMS_GOLDEN=<file> writes `name\trh\tch` per
        // cell + the digest; LCMS_GOLDEN_UPDATE=1 regenerates after a
        // canonical-math swap. Compares and reports drift if it exists.
        if let Ok(g) = std::env::var("LCMS_GOLDEN") {
            let mut txt = format!("DIGEST {d:016x}
");
            for (n, r, c) in &cells {
                txt += &format!("{n}\t{:016x}\t{:016x}\n", fnv(r), fnv(c));
            }
            let upd = std::env::var("LCMS_GOLDEN_UPDATE").is_ok();
            let old = std::fs::read_to_string(&g).ok();
            match (old, upd) {
                (_, true) => { std::fs::write(&g, &txt).unwrap();
                    eprintln!("GOLDEN wrote {g}"); }
                (Some(o), false) if o != txt => {
                    let on: std::collections::HashMap<_, _> = o.lines().skip(1)
                        .filter_map(|l| {
                            let mut it = l.split('\t');
                            it.next().zip(it.next())
                        }).collect();
                    for (n, r, _c) in &cells {
                        let rh = format!("{:016x}", fnv(r));
                        if let Some(&or) = on.get(n.as_str()) {
                            if or != rh {
                                eprintln!("GOLDEN-DRIFT {n}: {or} -> {rh}");
                            }
                        }
                    }
                    eprintln!("GOLDEN differs (LCMS_GOLDEN_UPDATE=1 to refresh)");
                }
                (None, false) => { std::fs::write(&g, &txt).unwrap();
                    eprintln!("GOLDEN seeded {g}"); }
                _ => eprintln!("GOLDEN ok"),
            }
        }
    }
}

/// The shared-plan model: `TransformPlan` is `Sync` + `Send` (Arc-able,
/// evaluated from many threads); `Transform` is `Send`. Compile-time proof.
#[test]
fn plan_sync_send() {
    fn assert_sync<T: Sync>() {}
    fn assert_send<T: Send>() {}
    assert_sync::<xform::TransformPlan>();
    assert_send::<xform::TransformPlan>();
    assert_send::<xform::Transform>();
    assert_send::<xform::TransformScratch>();
}

/// Frozen profiles are Sync-shareable for reads.
#[test]
fn profile_freeze_sync() {
    fn assert_send<T: Send>() {}
    assert_send::<profile::IccProfile>();
    // freeze a real profile then read via the frozen path
    let mut p = virt::create_srgb_profile(None).unwrap();
    profile::freeze(&mut p);
    assert!(profile::read_tag_frozen(&p, 0x72545243).is_some()); // rTRC
}

/// api layer smoke: Profile + Plan + Scratch, srgb->lab4 round trip
#[test]
fn api_smoke() {
    use lcms::api::*;
    let mut srgb = Profile::srgb().unwrap();
    let mut lab4 = Profile::lab4().unwrap();
    srgb.freeze();
    lab4.freeze();
    let plan = Plan::new(&mut srgb, &mut lab4, Format::RgbU8, Format::LabU8).unwrap();
    let mut sc = plan.scratch();
    let src = [255u8, 0, 0]; // pure red
    let mut dst = [0u8; 3];
    plan.apply_n(&mut sc, &src, &mut dst, 1);
    // Lab L* > 0 for red
    assert!(dst[0] > 0, "red -> Lab L* nonzero: {:?}", dst);
}

/// api::Error carries the real signal_error text when a path signals.
#[test]
fn api_error_real_message() {
    use lcms::api::*;
    let e = Profile::open(&[0u8; 64]).unwrap_err();
    // open_profile_from_mem signals a corruption/unsupported error —
    // the message should come from the error handler, not our fallback
    assert!(!e.message.is_empty());
    eprintln!("api err: {} (id {})", e.message, e.id);
}
