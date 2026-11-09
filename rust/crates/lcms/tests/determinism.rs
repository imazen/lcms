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
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// zero the volatile header fields (creation datetime, profile ID)
fn mask(b: &mut Vec<u8>) {
    for i in 0..12 { b.get_mut(24 + i).map(|x| *x = 0); }
    for i in 0..16 { b.get_mut(84 + i).map(|x| *x = 0); }
}

fn emit(name: &str, r: &[u8], cb: &[u8]) {
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
    let mut buf = vec![0u8; 1 << 20];
    let mut n = buf.len() as u32;
    if c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut n) == 0 {
        return Vec::new();
    }
    buf.truncate(n as usize);
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
    let mut rop = if out_b.is_empty() { None }
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
    }
}

/// debug: first-diverging pixels on xfl srgb>lab4
#[test]
fn dbg_xfl_lab() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        let l = c::cmsCreateLab4Profile(std::ptr::null());
        let t = c::cmsCreateTransform(p, ff32(4,3), l, ff32(10,3), 0, 0);
        let inp: [f32;3] = [0.5, 0.5, 0.5];
        let mut oc = [0.0f32;3];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
        let mut rp = profile::open_profile_from_mem(None, &save_c(p)).unwrap();
        let mut rl = profile::open_profile_from_mem(None, &save_c(l)).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rp, rl];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, ff32(4,3), ff32(10,3), 0).unwrap();
        let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 12);
        let mut ob = [0u8;12];
        rt.do_transform(ib, &mut ob, 1);
        let ro: &[f32] = std::slice::from_raw_parts(ob.as_ptr() as *const f32, 3);
        eprintln!("C: {:?}\nR: {:?}", oc, ro);
        c::cmsCloseProfile(p); c::cmsCloseProfile(l); c::cmsDeleteTransform(t);
    }
}

/// bisect: srgb->XYZ float — does the divergence enter at input path or PCS?
#[test]
fn dbg_xfl_xyz() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        let l = c::cmsCreateXYZProfile();
        let pb = save_c(p); let lb = save_c(l);
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let l2 = c::cmsOpenProfileFromMem(lb.as_ptr() as *const _, lb.len() as u32);
        let t = c::cmsCreateTransform(p2, ff32(4,3), l2, ff32(9,3), 0, 0);
        let inp: [f32;3] = [0.5, 0.5, 0.5];
        let mut oc = [0.0f32;3];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
        let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
        let mut rl = profile::open_profile_from_mem(None, &lb).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rp, rl];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, ff32(4,3), ff32(9,3), 0).unwrap();
        let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 12);
        let mut ob = [0u8;12];
        rt.do_transform(ib, &mut ob, 1);
        let ro: &[f32] = std::slice::from_raw_parts(ob.as_ptr() as *const f32, 3);
        eprintln!("C XYZ: {:?}\nR XYZ: {:?}", oc, ro);
        // also compare manual pipeline eval: read Rust profile's A2B0 chain
        c::cmsCloseProfile(p); c::cmsCloseProfile(l); c::cmsDeleteTransform(t);
    }
}

/// bisect2: does C's optimizer explain the Lab float gap?
#[test]
fn dbg_xfl_noopt() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        let l = c::cmsCreateLab4Profile(std::ptr::null());
        let pb = save_c(p); let lb = save_c(l);
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let l2 = c::cmsOpenProfileFromMem(lb.as_ptr() as *const _, lb.len() as u32);
        for flags in [0u32, 0x0040 /*NOOPT*/, 0x4000000 /*COPY_ALPHA?*/] {
            let t = c::cmsCreateTransform(p2, ff32(4,3), l2, ff32(10,3), 0, flags);
            let inp: [f32;3] = [0.5, 0.5, 0.5];
            let mut oc = [0.0f32;3];
            c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
            eprintln!("C flags={flags:#x}: {oc:?}");
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
        let mut rl = profile::open_profile_from_mem(None, &lb).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rp, rl];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, ff32(4,3), ff32(10,3), 0).unwrap();
        let inp: [f32;3] = [0.5, 0.5, 0.5];
        let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 12);
        let mut ob = [0u8;12];
        rt.do_transform(ib, &mut ob, 1);
        let ro: &[f32] = std::slice::from_raw_parts(ob.as_ptr() as *const f32, 3);
        eprintln!("R           : {ro:?}");
        c::cmsCloseProfile(p); c::cmsCloseProfile(l);
        c::cmsCloseProfile(p2); c::cmsCloseProfile(l2);
    }
}

/// bisect3: labdl float — measure actual per-pixel diff pattern
#[test]
fn dbg_xfl_labdl() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        c::cmsSetDeviceClass(p, 0x6C696E6B);
        c::cmsSetColorSpace(p, 0x4C616220);
        c::cmsSetPCS(p, 0x4C616220);
        c::cmsSetProfileVersion(p, 2.1);
        c::cmsWriteTag(p, 0x41324230, c_mft2_pipeline(3, 3));
        let wt2 = *c::cmsD50_XYZ();
        c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _);
        let pb = save_c(p);
        eprintln!("labdl size {}", pb.len());
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let mut arr = [p2];
        let t = c::cmsCreateMultiprofileTransform(arr.as_mut_ptr(), 1, ff32(10,3), ff32(10,3), 0, 0);
        for inp in [[50.0f32, 0.5, -0.3], [25.0, 10.0, -10.0], [80.0, -50.0, 50.0]] {
            let mut oc = [0.0f32;3];
            c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
            let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
            let mut profs: Vec<Box<IccProfile>> = vec![rp];
            let mut bpc = vec![false]; let ints = vec![0u32]; let ad = vec![1.0];
            let mut rt = xform::create_extended_transform(None, &mut profs,
                &mut bpc, &ints, &ad, None, 0, ff32(10,3), ff32(10,3), 0).unwrap();
            let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 12);
            let mut ob = [0u8;12];
            rt.do_transform(ib, &mut ob, 1);
            let ro: &[f32] = std::slice::from_raw_parts(ob.as_ptr() as *const f32, 3);
            eprintln!("in {inp:?}  C {oc:?}  R {ro:?}");
        }
        c::cmsCloseProfile(p); c::cmsCloseProfile(p2); c::cmsDeleteTransform(t);
    }
}

/// bisect4: which call fails on the mab placeholder?
#[test]
fn dbg_mab_fail() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        eprintln!("ph {:p}", p);
        c::cmsSetDeviceClass(p, 0x6D6E7472);
        c::cmsSetColorSpace(p, 0x52474220);
        c::cmsSetPCS(p, 0x4C616220);
        c::cmsSetProfileVersion(p, 4.4);
        let lut = c_mpet_pipeline(3, 3);
        eprintln!("lut {:p}", lut);
        eprintln!("wtA2B0 {}", c::cmsWriteTag(p, 0x41324230, lut));
        let wt2 = *c::cmsD50_XYZ();
        eprintln!("wtwtpt {}", c::cmsWriteTag(p, 0x77747074, &wt2 as *const _ as *const _));
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(), b"US\0".as_ptr(), b"x\0".as_ptr());
        eprintln!("wtdesc {}", c::cmsWriteTag(p, 0x64657363, mlu));
        let b = save_c(p);
        eprintln!("saved {} bytes", b.len());
        if !b.is_empty() {
            let p2 = c::cmsOpenProfileFromMem(b.as_ptr() as *const _, b.len() as u32);
            eprintln!("reopen {:p}", p2);
        }
        c::cmsCloseProfile(p);
    }
}

/// bisect5: which pipeline element breaks mAB save?
#[test]
fn dbg_mab_elems() {
    unsafe {
        let ctx = std::ptr::null_mut();
        for name in ["cvst", "clutf", "matf"] {
            let p = c::cmsCreateProfilePlaceholder(ctx);
            c::cmsSetDeviceClass(p, 0x6D6E7472);
            c::cmsSetColorSpace(p, 0x52474220);
            c::cmsSetPCS(p, 0x4C616220);
            c::cmsSetProfileVersion(p, 4.4);
            let lut = c::cmsPipelineAlloc(ctx, 3, 3);
            match name {
                "cvst" => {
                    let g = c::cmsBuildGamma(ctx, 1.8);
                    let cv = c::cmsStageAllocToneCurves(ctx, 3, [g,g,g].as_ptr());
                    c::cmsPipelineInsertStage(lut, 1, cv);
                }
                "clutf" => {
                    let mut tab = vec![0.0f32; 216*3];
                    for (i,x) in tab.iter_mut().enumerate() { *x = (i%64) as f32/64.0; }
                    let cl = c::cmsStageAllocCLutFloat(ctx, 6, 3, 3, tab.as_ptr());
                    c::cmsPipelineInsertStage(lut, 1, cl);
                }
                "matf" => {
                    let m: [f64;9] = [1.,0.,0.,0.,1.,0.,0.,0.,1.];
                    let o: [f64;3] = [0.;3];
                    let mt = c::cmsStageAllocMatrix(ctx, 3, 3, m.as_ptr(), o.as_ptr());
                    c::cmsPipelineInsertStage(lut, 1, mt);
                }
                _ => {}
            }
            eprintln!("{name} A2B0={} save={}", c::cmsWriteTag(p, 0x41324230, lut), save_c(p).len());
            c::cmsCloseProfile(p);
        }
    }
}

/// bisect6: x16 mab — actual u16 pixel diffs
#[test]
fn dbg_x16_mab() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        c::cmsSetDeviceClass(p, 0x6D6E7472);
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
        let pb = save_c(p);
        eprintln!("mab size {}", pb.len());
        let sr = c::cmsCreate_sRGBProfile();
        let sb = save_c(sr);
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let s2 = c::cmsOpenProfileFromMem(sb.as_ptr() as *const _, sb.len() as u32);
        let t = c::cmsCreateTransform(p2, f16(4,3), s2, f16(4,3), 0, 0);
        for inp in [[0u16,0,0],[32768u16,32768,32768],[65535,32768,16384],[16384,49152,32768]] {
            let mut oc = [0u16;3];
            c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
            let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
            let mut rs = profile::open_profile_from_mem(None, &sb).unwrap();
            let mut rp2 = profile::open_profile_from_mem(None, &pb).unwrap();
            let mut rs2 = profile::open_profile_from_mem(None, &sb).unwrap();
            let mut profs2: Vec<Box<IccProfile>> = vec![rp2, rs2];
            let mut bpc2 = vec![false;2]; let ints2 = vec![0u32;2]; let ad2 = vec![1.0;2];
            let mut profs: Vec<Box<IccProfile>> = vec![rp, rs];
            let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
            for flags in [0u32, 0x0040] {
                let mut rt = xform::create_extended_transform(None, &mut profs2,
                    &mut bpc2, &ints2, &ad2, None, 0, f16(4,3), f16(4,3), flags).unwrap();
                let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 6);
                let mut ob = [0u8;6];
                rt.do_transform(ib, &mut ob, 1);
                let ro: &[u16] = std::slice::from_raw_parts(ob.as_ptr() as *const u16, 3);
                eprintln!("  R flags={flags:#x} {ro:?}");
            }
            let mut rt = xform::create_extended_transform(None, &mut profs,
                &mut bpc, &ints, &ad, None, 0, f16(4,3), f16(4,3), 0).unwrap();
            let ib: &[u8] = std::slice::from_raw_parts(inp.as_ptr() as *const u8, 6);
            let mut ob = [0u8;6];
            rt.do_transform(ib, &mut ob, 1);
            let ro: &[u16] = std::slice::from_raw_parts(ob.as_ptr() as *const u16, 3);
            eprintln!("in {inp:?}  C {oc:?}  R {ro:?}");
        }
        c::cmsCloseProfile(p); c::cmsCloseProfile(p2); c::cmsCloseProfile(s2); c::cmsDeleteTransform(t);
    }
}

/// bisect7: find first diverging pixel in x16 mab sweep
#[test]
fn dbg_x16_mab_sweep() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        c::cmsSetDeviceClass(p, 0x6D6E7472);
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
        let chad: [f64;9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
        let pb = save_c(p);
        let sr = c::cmsCreate_sRGBProfile();
        let sb = save_c(sr);
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let s2 = c::cmsOpenProfileFromMem(sb.as_ptr() as *const _, sb.len() as u32);
        let t = c::cmsCreateTransform(p2, f16(4,3), s2, f16(4,3), 0, 0);
        let sw = sweep16(3);
        let np = sw.len() / 3;
        let mut cb = vec![0u8; np * 6];
        c::cmsDoTransform(t, sw.as_ptr() as *const _, cb.as_mut_ptr() as *mut _, np as u32);
        let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
        let mut rs = profile::open_profile_from_mem(None, &sb).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rp, rs];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, f16(4,3), f16(4,3), 0).unwrap();
        let mut rb = vec![0u8; np * 6];
        rt.do_transform(byt2(&sw), &mut rb, np as u32);
        let cf: &[u16] = std::slice::from_raw_parts(cb.as_ptr() as *const u16, np*3);
        let rf: &[u16] = std::slice::from_raw_parts(rb.as_ptr() as *const u16, np*3);
        let mut shown = 0;
        for px in 0..np {
            let d: Vec<i32> = (0..3).map(|k| rf[px*3+k] as i32 - cf[px*3+k] as i32).collect();
            if d.iter().any(|&x| x != 0) && shown < 6 {
                eprintln!("px{px} in={:?} C={:?} R={:?}",
                    &sw[px*3..px*3+3], &cf[px*3..px*3+3], &rf[px*3..px*3+3]);
                shown += 1;
            }
        }
        eprintln!("total differing: {}", (0..np).filter(|&px| rf[px*3..px*3+3] != cf[px*3..px*3+3]).count());
    }
}

/// bisect8: pipeline-level eval of the read mAB lut on black
#[test]
fn dbg_mab_pipeline() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        c::cmsSetDeviceClass(p, 0x6D6E7472);
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
        let chad: [f64;9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        c::cmsWriteTag(p, 0x63686164, chad.as_ptr() as *const _);
        let pb = save_c(p);
        let p2 = c::cmsOpenProfileFromMem(pb.as_ptr() as *const _, pb.len() as u32);
        let lut = c::cmsReadTag(p2, 0x41324230);
        eprintln!("clut {:p}", lut);
        for inp in [[0u16,0,0],[32768,32768,32768],[65535,32768,16384]] {
            let mut oc = [0u16;3];
            c::cmsPipelineEval16(inp.as_ptr(), oc.as_mut_ptr(), lut);
            let mut of = [0f32;3];
            let inf = [inp[0] as f32/65535.0, inp[1] as f32/65535.0, inp[2] as f32/65535.0];
            c::cmsPipelineEvalFloat(inf.as_ptr(), of.as_mut_ptr(), lut);
            // rust side
            let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
            // read the a2b0 tag pipeline — same public API used by xform
            let r16 = {
                let mut o = [0u16;3];
                if let Some(lcms::tagtypes::TagObject::LutA2B(lut)) =
                    profile::cms_read_tag(&mut rp, 0x41324230) {
                    lut.eval16(&inp, &mut o);
                }
                o
            };
            eprintln!("in {inp:?} C16={oc:?} Cf={of:?} R16={r16:?}");
        }
        c::cmsCloseProfile(p); c::cmsCloseProfile(p2);
    }
}

/// bisect9: dump the resampled CLUT node(0,0,0) + first nodes
#[test]
fn dbg_resampled_clut() {
    unsafe {
        let ctx = std::ptr::null_mut();
        let p = c::cmsCreateProfilePlaceholder(ctx);
        c::cmsSetDeviceClass(p, 0x6D6E7472);
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
        let pb = save_c(p);
        let sr = c::cmsCreate_sRGBProfile();
        let sb = save_c(sr);
        let mut rp = profile::open_profile_from_mem(None, &pb).unwrap();
        let mut rs = profile::open_profile_from_mem(None, &sb).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rp, rs];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, f16(4,3), f16(4,3), 0).unwrap();
        // dump the optimized pipeline's stages + first clut entries
        if let Some(lut) = &rt.lut {
            let mut st = lut.elements.as_deref();
            let mut i = 0;
            while let Some(s) = st {
                eprintln!("stage{i} {:?}", s.stage_type());
                if let lcms::lut::StageData::CLut(d) = &s.data {
                    eprintln!("  clut tab[0..12] = {:?}", &d.tab16.as_deref().unwrap_or(&[])[..12.min(d.tab16.as_deref().map(|x| x.len()).unwrap_or(0))]);
                }
                st = s.next.as_deref();
                i += 1;
            }
            eprintln!("  opt_data = {:?}", lut.opt_data.as_ref().map(|d| std::mem::discriminant(d)));

            // generic eval16 of the resampled pipeline at vertex 0
            let mut o = [0u16;3];
            lut.eval16(&[0u16,0,0], &mut o);
            eprintln!("  generic eval16(0) = {o:?}");
            let mut o2 = [0u16;3];
            lut.eval16(&[65535u16,0,0], &mut o2);
            eprintln!("  generic eval16(65535,0,0) = {o2:?}");
        }
    }
}

/// bisect10: lab4 B2A0 pipeline eval — same XYZ input both sides
#[test]
fn dbg_lab4_b2a0() {
    unsafe {
        let l = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lb = save_c(l);
        let l2 = c::cmsOpenProfileFromMem(lb.as_ptr() as *const _, lb.len() as u32);
        let lut = c::cmsReadTag(l2, 0x42324130); // B2A0
        eprintln!("B2A0 lut {:p}", lut);
        for inp in [[0.5f32,0.5,0.5],[0.25,0.25,0.25],[0.1,0.11,0.12]] {
            let mut oc = [0f32;3];
            c::cmsPipelineEvalFloat(inp.as_ptr(), oc.as_mut_ptr(), lut);
            let mut rl = profile::open_profile_from_mem(None, &lb).unwrap();
            let ro = {
                let mut o = [0f32;3];
                if let Some(lcms::tagtypes::TagObject::LutB2A(lut)) =
                    profile::cms_read_tag(&mut rl, 0x42324130) {
                    lut.eval_float(&inp, &mut o);
                }
                o
            };
            eprintln!("in {inp:?} C={oc:?} R={ro:?}");
        }
    }
}

/// bisect11: which opt path does srgb>lab4 float take?
#[test]
fn dbg_lab4_opt() {
    unsafe {
        let s = c::cmsCreate_sRGBProfile();
        let l = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let sb = save_c(s); let lb = save_c(l);
        let mut rs = profile::open_profile_from_mem(None, &sb).unwrap();
        let mut rl = profile::open_profile_from_mem(None, &lb).unwrap();
        let mut profs: Vec<Box<IccProfile>> = vec![rs, rl];
        let mut bpc = vec![false;2]; let ints = vec![0u32;2]; let ad = vec![1.0;2];
        // TYPE_RGB_FLT->TYPE_Lab_FLT
        let ifmt = ff32(4,3); let ofmt = ff32(10,3);
        let mut rt = xform::create_extended_transform(None, &mut profs,
            &mut bpc, &ints, &ad, None, 0, ifmt, ofmt, 0).unwrap();
        if let Some(lut) = &rt.lut {
            eprintln!("opt={:?} stages:", lut.opt_data.as_ref().map(|d| std::mem::discriminant(d)));
            let mut st = lut.elements.as_deref();
            let mut i = 0;
            while let Some(s) = st {
                eprintln!("  st{i} {:#x}", s.stage_type());
                st = s.next.as_deref(); i += 1;
            }
        }
        // and eval one pixel through the transform vs the C oracle
        let s2 = c::cmsOpenProfileFromMem(sb.as_ptr() as *const _, sb.len() as u32);
        let l2 = c::cmsOpenProfileFromMem(lb.as_ptr() as *const _, lb.len() as u32);
        let t = c::cmsCreateTransform(s2, ff32(4,3), l2, ff32(10,3), 0, 0);
        let inp = [0.5f32,0.5,0.5];
        let mut oc = [0f32;3];
        c::cmsDoTransform(t, inp.as_ptr() as *const _, oc.as_mut_ptr() as *mut _, 1);
        let mut ob = [0u8;12];
        rt.do_transform(unsafe { std::slice::from_raw_parts(inp.as_ptr() as *const u8, 12) }, &mut ob, 1);
        let ro: &[f32] = unsafe { std::slice::from_raw_parts(ob.as_ptr() as *const f32, 3) };
        eprintln!("C={oc:?} R={ro:?}");
    }
}

/// bisect12: C-side link stage types for srgb>lab4
#[test]
fn dbg_c_link_stages() {
    unsafe {
        let s = c::cmsCreate_sRGBProfile();
        let l = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let sb = save_c(s); let lb = save_c(l);
        let s2 = c::cmsOpenProfileFromMem(sb.as_ptr() as *const _, sb.len() as u32);
        let l2 = c::cmsOpenProfileFromMem(lb.as_ptr() as *const _, lb.len() as u32);
        let t = c::cmsCreateTransform(s2, ff32(4,3), l2, ff32(10,3), 0, 0);
        let lut = c::cref_xform_lut(t);
        let mut st = c::cmsPipelineGetPtrToFirstStage(lut);
        let mut i = 0;
        while !st.is_null() {
            eprintln!("C st{i} {:#x}", c::cmsStageType(st));
            st = c::cmsStageNext(st);
            i += 1;
        }
    }
}
