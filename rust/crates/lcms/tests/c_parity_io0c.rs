//! cmsio0 file/IOhandler paths: SaveProfileToFile + OpenProfileFromFile
//! round-trip on the corpus, GetHeaderRenderingIntent parity.

use lcms::profile;
use lcms_cref as c;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn file_open_save_roundtrip() {
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        let tmp = std::env::temp_dir().join(format!("lcms_{}.icc", rp.profile_id.id8[0]));
        let c_ok = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            let name = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();
            let ok = c::cmsSaveProfileToFile(h, name.as_ptr());
            c::cmsCloseProfile(h);
            ok
        };
        let r_bytes = profile::save_profile_to_mem(&mut rp).unwrap_or_default();
        assert_eq!(c_ok != 0, !r_bytes.is_empty(), "save status");
        if c_ok != 0 {
            // C's file save vs rust's mem save — the file content should
            // match the mem bytes (same serialization path)
            let c_bytes = std::fs::read(&tmp).unwrap_or_default();
            let mut c_out = c_bytes;
            let mut r_out = r_bytes;
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "file-vs-mem bytes");
            // open the saved file on both sides — header parity
            let (c_ok2, c_flags) = unsafe {
                let name = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();
                let h = c::cmsOpenProfileFromFile(name.as_ptr(), b"r\0".as_ptr());
                if h.is_null() { (false, 0) } else {
                    let mut f = 0u32;
                    c::cmsGetHeaderFlags(h, &mut f);
                    c::cmsCloseProfile(h);
                    (true, f)
                }
            };
            let rrp = profile::open_profile_from_mem(None, &std::fs::read(&tmp).unwrap());
            assert_eq!(c_ok2, rrp.is_some(), "reopen presence");
            if let Some(rrp) = rrp {
                assert_eq!(c_flags, rrp.flags, "reopen flags");
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

#[test]
fn rendering_intent_getter() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        let c_intent = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            let i = c::cmsGetHeaderRenderingIntent(h);
            c::cmsCloseProfile(h);
            i
        };
        assert_eq!(c_intent, rp.rendering_intent, "rendering_intent");
    }
}

/// cmsio1 residual — XYZ-colorspace float INPUT tag
///   -> NormalizeToXyzFloat input-normalize stage.
#[test]
fn internal_xyz_float_in_arms() {
    unsafe {
        // XYZ-colorspace profile with a float A2B0 tag
        let p = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x73706163);
        c::cmsSetColorSpace(p, 0x58595A20); // XYZ
        c::cmsSetPCS(p, 0x58595A20);       // XYZ
        c::cmsSetProfileVersion(p, 4.4);
        // float pipeline: identity-ish CLUT float
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 3, 3);
        let dims = [2u32; 15];
        let grid = vec![
            0.5f32, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
            0.5, 0.5, 0.5,
        ];
        c::cmsPipelineInsertStage(pipe, 0,
            c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), dims.as_ptr(),
                3, 3, grid.as_ptr()));
        c::cmsWriteTag(p, 0x41324642,
            pipe as *const _); // A2B0 float 'D2B0'?
        // try D2B0 float tag too
        c::cmsWriteTag(p, 0x44324230,
            pipe as *const _);
        // transform: input = xyz profile, output = srgb
        let srgb = c::cmsCreate_sRGBProfile();
        let fin = (1u32 << 22) | (10 << 16) | (3 << 3) | 4;
        let fout = (0u32 << 16) | (3 << 3) | 1;
        for int_ in [0u32, 1] {
            let t = c::cmsCreateTransform(p, fin,
                srgb, fout, int_, 0);
            eprintln!("xyz-in int{int_} t={}",
                !t.is_null());
            if !t.is_null() {
                let src = [0.5f32, 0.5, 0.5];
                let mut dst = [0u8; 4];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 1);
                eprintln!("  {src:?}->{dst:?}");
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(p);
        c::cmsCloseProfile(srgb);
        c::cmsPipelineFree(pipe);
    }
}

/// cmsio1 residual — BuildRGBOutputMatrixShaper /
///   BuildGrayOutputPipeline via output-direction
///   transforms (srgb->lab, lab->srgb, gray->lab)
///   on matrix-shaper + gray profiles, all intents.
#[test]
fn internal_matshaper_out_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(
            std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfileTHR(
            std::ptr::null_mut(), std::ptr::null(),
            g as *const std::ffi::c_void);
        let frgb = (0u32 << 16) | (3 << 3) | 2;
        let flab = (0u32 << 16) | (3 << 3) | 2;
        let fgray = (0u32 << 16) | (1 << 3) | 2;
        // output direction: PCS -> device
        for (inp, ifmt, out, ofmt, name) in [
            (lab4, flab, srgb, frgb, "lab4->srgb"),
            (lab2, flab, srgb, frgb, "lab2->srgb"),
            (xyz, flab, srgb, frgb, "xyz->srgb"),
            (srgb, frgb, lab4, flab, "srgb->lab4"),
            (srgb, frgb, xyz, flab, "srgb->xyz"),
            (lab4, flab, gray, fgray, "lab4->gray"),
            (xyz, flab, gray, fgray, "xyz->gray"),
            (gray, fgray, lab4, flab, "gray->lab4"),
            (gray, fgray, srgb, frgb, "gray->srgb"),
        ] {
            for int_ in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(inp, ifmt,
                    out, ofmt, int_, 0);
                if !t.is_null() {
                    let src = [0x8000u16; 4];
                    let mut dst = [0u16; 4];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    if int_ == 0 {
                        eprintln!("{name} ok {dst:?}"); }
                    c::cmsDeleteTransform(t);
                } else if int_ == 0 {
                    eprintln!("{name} t=0"); }
            }
        }
        // BPC + gamut + proofing flags on output dirs
        for fl in [0x4000u32, 0x0040, 0x0100, 0x2000] {
            let t = c::cmsCreateTransform(lab4, flab,
                srgb, frgb, 1, fl);
            eprintln!("lab4->srgb fl{fl:#x} t={}",
                !t.is_null());
            if !t.is_null() {
                c::cmsDeleteTransform(t); }
        }
        for p in [srgb, lab4, lab2, xyz, gray] {
            if !p.is_null() {
                c::cmsCloseProfile(p); } }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsio1 residual — _cmsReadInputLUT Lab-colorspace
///   + mft1-type A2B0 -> LabV4ToV2 + LabV2ToV4 stage
///   inserts (v2 Lab device profile).
#[test]
fn internal_lab_mft_in_arms() {
    unsafe {
        // v2 profile: colorspace=Lab, PCS=Lab, mft1 A2B0
        let p = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x70727472);
        c::cmsSetColorSpace(p, 0x4C616220);
        c::cmsSetPCS(p, 0x4C616220);
        c::cmsSetProfileVersion(p, 2.4);
        // mft1 tag: in3 out3 grid2 + matrix + tables
        let mut blob = vec![
            0x6D, 0x66, 0x74, 0x31, 0, 0, 0, 0,
            3, 3, 2, 0,
        ];
        for i in 0..9 {
            let v: u32 = if i % 4 == 0 {
                0x00010000 } else { 0 };
            blob.extend_from_slice(&v.to_be_bytes());
        }
        blob.extend_from_slice(&[0x80u8; 256 * 3]);
        blob.extend_from_slice(&[0x80u8; 8 * 3]);
        blob.extend_from_slice(&[0x80u8; 256 * 3]);
        c::cmsWriteRawTag(p, 0x41324230,
            blob.as_ptr() as *const _, blob.len() as u32);
        let mut buf = [0u8; 1 << 17];
        let mut n = buf.len() as u32;
        c::cmsSaveProfileToMem(p,
            buf.as_mut_ptr() as *mut _, &mut n);
        let pr = c::cmsOpenProfileFromMem(
            buf.as_ptr() as *const _, n);
        eprintln!("v2-lab-mft prof={}", !pr.is_null());
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let fin = (10u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransform(pr, fin,
            lab4, fin, 0, 0);
        eprintln!("  t={}", !t.is_null());
        if !t.is_null() {
            let src = [0x8000u16; 3];
            let mut dst = [0u16; 3];
            c::cmsDoTransform(t,
                src.as_ptr() as *const _,
                dst.as_mut_ptr() as *mut _, 1);
            eprintln!("  {src:?}->{dst:?}");
            c::cmsDeleteTransform(t);
        }
        // mft2 variant too
        let p2 = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(p2, 0x70727472);
        c::cmsSetColorSpace(p2, 0x4C616220);
        c::cmsSetPCS(p2, 0x4C616220);
        c::cmsSetProfileVersion(p2, 2.4);
        let mut b2 = vec![
            0x6D, 0x66, 0x74, 0x32, 0, 0, 0, 0,
            3, 3, 2, 0,
        ];
        for i in 0..9 {
            let v: u32 = if i % 4 == 0 {
                0x00010000 } else { 0 };
            b2.extend_from_slice(&v.to_be_bytes());
        }
        b2.extend_from_slice(&[0u8; 2]); // in-n u16
        b2.extend_from_slice(&[0x02, 0x00]); // in sz
        // mft2: nInputEntries u16, nOutputEntries u16
        //   then in-table n*3 u16, clut, out-table
        //   actually layout: u16 in-entries, out-entries
        let mut b3 = vec![
            0x6D, 0x66, 0x74, 0x32, 0, 0, 0, 0,
            3, 3, 2, 0,
        ];
        for i in 0..9 {
            let v: u32 = if i % 4 == 0 {
                0x00010000 } else { 0 };
            b3.extend_from_slice(&v.to_be_bytes());
        }
        b3.extend_from_slice(&[0x01, 0x00]); // in=256
        b3.extend_from_slice(&[0x01, 0x00]); // out=256
        for _ in 0..(256 * 3) {
            b3.extend_from_slice(&0x8000u16
                .to_be_bytes()); }
        for _ in 0..(8 * 3) {
            b3.extend_from_slice(&0x8000u16
                .to_be_bytes()); }
        for _ in 0..(256 * 3) {
            b3.extend_from_slice(&0x8000u16
                .to_be_bytes()); }
        let _ = b2;
        c::cmsWriteRawTag(p2, 0x41324230,
            b3.as_ptr() as *const _, b3.len() as u32);
        let mut n2 = buf.len() as u32;
        c::cmsSaveProfileToMem(p2,
            buf.as_mut_ptr() as *mut _, &mut n2);
        let pr2 = c::cmsOpenProfileFromMem(
            buf.as_ptr() as *const _, n2);
        eprintln!("v2-lab-mft2 prof={}", !pr2.is_null());
        let t2 = c::cmsCreateTransform(pr2, fin,
            lab4, fin, 0, 0);
        eprintln!("  t2={}", !t2.is_null());
        if !t2.is_null() {
            c::cmsDeleteTransform(t2); }
        if !pr.is_null() { c::cmsCloseProfile(pr); }
        if !pr2.is_null() { c::cmsCloseProfile(pr2); }
        c::cmsCloseProfile(p);
        c::cmsCloseProfile(p2);
        c::cmsCloseProfile(lab4);
    }
}

/// cmsio1 residual — _cmsReadFloat{Input,Output,
///   Devicelink}Tag: mpet DToB*/BToD*/A2B0/B2A0
///   float pipelines on input, output and devicelink
///   profiles through transforms.
#[test]
fn internal_floatlut_arms() {
    unsafe {
        let mpet = |nch: u16| {
            // single cvst element pipeline
            let mut cv = vec![
                0x63u8, 0x76, 0x73, 0x74, 0, 0, 0, 0];
            cv.extend_from_slice(&nch.to_be_bytes());
            cv.extend_from_slice(&nch.to_be_bytes());
            let csz = 40usize;
            let base = 8 + 4 + 8 * nch as usize;
            for i in 0..nch as usize {
                cv.extend_from_slice(
                    &((base + i * csz) as u32)
                        .to_be_bytes());
                cv.extend_from_slice(
                    &(csz as u32).to_be_bytes());
            }
            for _ in 0..nch {
                cv.extend_from_slice(&[
                    0x63, 0x75, 0x72, 0x66, 0, 0, 0, 0,
                    0, 1, 0, 0,
                    0x70, 0x61, 0x72, 0x66, 0, 0, 0, 0,
                    0, 0, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                ]);
            }
            let mut b = vec![
                0x6D, 0x70, 0x65, 0x74, 0, 0, 0, 0];
            b.extend_from_slice(&nch.to_be_bytes());
            b.extend_from_slice(&nch.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&24u32.to_be_bytes());
            b.extend_from_slice(
                &(cv.len() as u32).to_be_bytes());
            b.extend_from_slice(&cv);
            b
        };
        let mk = |cls: u32, cs: u32, pcs: u32,
                  tag: u32, nch: u16| {
            let p = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(p, cls);
            c::cmsSetColorSpace(p, cs);
            c::cmsSetPCS(p, pcs);
            c::cmsSetProfileVersion(p, 4.4);
            let blob = mpet(nch);
            c::cmsWriteRawTag(p, tag,
                blob.as_ptr() as *const _,
                blob.len() as u32);
            let mut buf = [0u8; 1 << 17];
            let mut n = buf.len() as u32;
            c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            let pr = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *const _, n);
            c::cmsCloseProfile(p);
            pr
        };
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // float INPUT tag: monitor with mpet A2B0
        let inp = mk(0x6D6E7472, 0x52474220,
            0x4C616220, 0x41324230, 3);
        eprintln!("mpet-in prof={}", !inp.is_null());
        // float OUTPUT tag: monitor with mpet B2A0
        let outp = mk(0x6D6E7472, 0x52474220,
            0x4C616220, 0x42324130, 3);
        eprintln!("mpet-out prof={}", !outp.is_null());
        // float DEVICELINK: link mpet DToB0 + BToD0
        let d2b = mk(0x6C696E6B, 0x52474220,
            0x4C616220, 0x44324230, 3);
        let b2d = mk(0x6C696E6B, 0x4C616220,
            0x52474220, 0x42324430, 3);
        eprintln!("mpet-d2b={} b2d={}",
            !d2b.is_null(), !b2d.is_null());
        // DToB0 on INPUT-class profile -> float-input
        let mntr_d2b = mk(0x6D6E7472, 0x52474220,
            0x4C616220, 0x44324230, 3);
        // BToD0 on OUTPUT-class profile -> float-out
        let mntr_b2d = mk(0x70727472, 0x52474220,
            0x4C616220, 0x42324430, 3);
        // also: mntr + DToB1/DToB2/DToB3 intents
        let mntr_d2b1 = mk(0x6D6E7472, 0x52474220,
            0x4C616220, 0x44324231, 3);
        eprintln!("mntr-d2b={} b2d={} d2b1={}",
            !mntr_d2b.is_null(), !mntr_b2d.is_null(),
            !mntr_d2b1.is_null());
        // transforms through each float-tag profile
        for (a, afmt, b, bfmt, name) in [
            (mntr_d2b, frgb, lab4, flab, "fd2b-in"),
            (lab4, flab, mntr_b2d, frgb, "fb2d-out"),
            (mntr_d2b1, frgb, lab4, flab, "fd2b1-in"),
            (inp, frgb, lab4, flab, "in->lab4"),
            (lab4, flab, outp, frgb, "lab4->out"),
            (d2b, frgb, lab4, flab, "d2b->lab4"),
            (lab4, flab, b2d, frgb, "lab4->b2d"),
            (srgb, frgb, inp, flab, "srgb->in"),
            (outp, flab, srgb, frgb, "out->srgb"),
            (inp, frgb, outp, frgb, "in->out"),
            (d2b, frgb, b2d, frgb, "d2b->b2d"),
        ] {
            let t = c::cmsCreateTransform(a, afmt,
                b, bfmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let src = [0x8000u16; 4];
                let mut dst = [0u16; 4];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        for p in [inp, outp, d2b, b2d, mntr_d2b,
                  mntr_b2d, mntr_d2b1, srgb, lab4] {
            if !p.is_null() {
                c::cmsCloseProfile(p); } }
    }
}

/// cmsio1 residual — BuildGrayInput/Output pipelines:
///   gray->lab, lab->gray, gray->srgb, srgb->gray,
///   gray->gray at intents + curve-gamma variants.
#[test]
fn internal_graypipe_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        for (gamma, name) in [
            (1.0f64, "g10"), (1.8, "g18"), (2.2, "g22"),
            (0.6, "g06"), (4.5, "g45"),
        ] {
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), gamma);
            let gray = c::cmsCreateGrayProfileTHR(
                std::ptr::null_mut(), std::ptr::null(),
                g as *const std::ffi::c_void);
            eprintln!("{name} gray={}", !gray.is_null());
            let fgray = (3u32 << 16) | (1 << 3) | 2;
            let flab = (10u32 << 16) | (3 << 3) | 2;
            let frgb = (4u32 << 16) | (3 << 3) | 2;
            for (a, af, b, bf, n) in [
                (gray, fgray, lab4, flab, "g->l"),
                (lab4, flab, gray, fgray, "l->g"),
                (gray, fgray, srgb, frgb, "g->r"),
                (srgb, frgb, gray, fgray, "r->g"),
                (gray, fgray, gray, fgray, "g->g"),
                (gray, fgray, xyz, flab, "g->x"),
                (xyz, flab, gray, fgray, "x->g"),
            ] {
                let t = c::cmsCreateTransform(a, af,
                    b, bf, 0, 0);
                if !t.is_null() {
                    let src = [0x8000u16; 4];
                    let mut dst = [0u16; 4];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    if gamma == 2.2 {
                        eprintln!("  {n} {src:?}->{dst:?}"); }
                    c::cmsDeleteTransform(t);
                }
            }
            // intents on the gray dirs
            for i in 0..4u32 {
                let t = c::cmsCreateTransform(gray, fgray,
                    srgb, frgb, i, 0);
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
            }
            if !gray.is_null() {
                c::cmsCloseProfile(gray); }
            c::cmsFreeToneCurve(g);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab4);
        c::cmsCloseProfile(xyz);
    }
}

/// cmsio1 residual — _cmsReadDevicelinkLUT intent
///   sweep + mft1/mft2/mAB/mBA devicelink tag shapes
///   through multiprofile chains.
#[test]
fn internal_devlink_lut_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // devicelink via mft1 A2B0 RGB->Lab
        let mk = |ty: u32| {
            let p = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(p, 0x6C696E6B);
            c::cmsSetColorSpace(p, 0x52474220);
            c::cmsSetPCS(p, 0x4C616220);
            c::cmsSetProfileVersion(p, 2.4);
            let mut blob = vec![
                0x6D, 0x66, 0x74, 0x31, 0, 0, 0, 0,
                3, 3, 2, 0,
            ];
            for i in 0..9 {
                let v: u32 = if i % 4 == 0 {
                    0x00010000 } else { 0 };
                blob.extend_from_slice(&v.to_be_bytes());
            }
            blob.extend_from_slice(&[0x80u8; 256 * 3]);
            blob.extend_from_slice(&[0x80u8; 8 * 3]);
            blob.extend_from_slice(&[0x80u8; 256 * 3]);
            c::cmsWriteRawTag(p, ty,
                blob.as_ptr() as *const _,
                blob.len() as u32);
            let mut buf = [0u8; 1 << 17];
            let mut n = buf.len() as u32;
            c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            let pr = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *const _, n);
            c::cmsCloseProfile(p);
            pr
        };
        // A2B0 + B2A0 + D2B0 + B2D0 devicelink shapes
        for (ty, name) in [
            (0x41324230u32, "A2B0"), (0x42324130, "B2A0"),
            (0x44324230, "D2B0"), (0x42324430, "B2D0"),
            (0x41324231, "A2B1"), (0x41324232, "A2B2"),
        ] {
            let dl = mk(ty);
            eprintln!("{name} prof={}", !dl.is_null());
            if !dl.is_null() {
                for int_ in [0u32, 1, 2, 3] {
                    let t = c::cmsCreateTransform(dl,
                        frgb, lab4, flab, int_, 0);
                    if !t.is_null() {
                        c::cmsDeleteTransform(t); }
                }
                c::cmsCloseProfile(dl);
            }
        }
        c::cmsCloseProfile(lab4);
    }
}

/// cmsio1 residual — BuildRGBOutputMatrixShaper on
///   output-class matrix profiles: non-identity
///   curves, Lab/XYZ PCS, custom primaries.
#[test]
fn internal_matshaper2_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let srgb0 = c::cmsCreate_sRGBProfile();
        let flab = (10u32 << 16) | (3 << 3) | 2;
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        // output-class RGB profiles w/ varying curves +
        //   primaries + PCS
        for (g_, wp_x, wp_y, pcs, name) in [
            (2.2f64, 0.3127, 0.3290, 0x58595A20u32,
                "srgb-like-xyz"),
            (2.2, 0.3127, 0.3290, 0x4C616220,
                "srgb-like-lab"),
            (1.0, 0.3127, 0.3290, 0x58595A20, "lin-xyz"),
            (1.8, 0.3477, 0.3595, 0x58595A20, "ado-xyz"),
            (2.6, 0.3140, 0.3510, 0x58595A20, "p3-xyz"),
            (2.2, 0.3457, 0.3585, 0x58595A20, "d50-xyz"),
        ] {
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), g_);
            let wp = c::CiexyY { x: wp_x, y: wp_y,
                Y: 1.0 };
            let prm = c::CiexyYTriple {
                Red: c::CiexyY { x: 0.64, y: 0.33,
                    Y: 1.0 },
                Green: c::CiexyY { x: 0.30, y: 0.60,
                    Y: 1.0 },
                Blue: c::CiexyY { x: 0.15, y: 0.06,
                    Y: 1.0 } };
            let trcs: [*mut std::ffi::c_void; 3] =
                [g as *mut _, g as *mut _, g as *mut _];
            let prof = c::cmsCreateRGBProfileTHR(
                std::ptr::null_mut(),
                &wp as *const _ as *const _,
                &prm as *const _ as *const _,
                trcs.as_ptr());
            eprintln!("{name} prof={}", !prof.is_null());
            // as output profile (pcs->device)
            for int_ in [0u32, 1] {
                let t = c::cmsCreateTransform(srgb0, frgb,
                    prof, frgb, int_, 0);
                eprintln!("  out i{int_} t={}",
                    !t.is_null());
                if !t.is_null() {
                    let src = [0x8000u16; 3];
                    let mut dst = [0u16; 3];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
                // as input (device->pcs)
                let t2 = c::cmsCreateTransform(prof, frgb,
                    lab4, flab, int_, 0);
                if !t2.is_null() {
                    c::cmsDeleteTransform(t2); }
            }
            let _ = pcs;
            if !prof.is_null() {
                c::cmsCloseProfile(prof); }
            c::cmsFreeToneCurve(g);
        }
        c::cmsCloseProfile(lab4);
        c::cmsCloseProfile(srgb0);
    }
}

/// cmsio1 residual — float-tag reader paths on
///   D2B*/B2D* mpet tags across all profile classes
///   + intent variants.
#[test]
fn internal_floattag2_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // build a 3->3 float cvst+clut+cvst pipeline
        let mk_pipe = || {
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), 1.0);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let dims = [3u32; 3];
            let grid = vec![0.5f32; 3usize.pow(3) * 3];
            c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocCLutFloatGranular(
                    std::ptr::null_mut(),
                    dims.as_ptr(), 3, 3, grid.as_ptr()));
            c::cmsPipelineInsertStage(pipe, 2,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            pipe
        };
        // write pipe into mpet tag on each class/tag
        for (cls, tag, name) in [
            (0x6D6E7472u32, 0x44324230u32, "mntr-D2B0"),
            (0x6D6E7472, 0x44324231, "mntr-D2B1"),
            (0x6D6E7472, 0x44324232, "mntr-D2B2"),
            (0x6D6E7472, 0x44324233, "mntr-D2B3"),
            (0x70727472, 0x42324430, "prtr-B2D0"),
            (0x70727472, 0x42324431, "prtr-B2D1"),
            (0x6C696E6B, 0x41324230, "link-A2B0"),
            (0x6C696E6B, 0x44324230, "link-D2B0"),
        ] {
            let p = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(p, cls);
            c::cmsSetColorSpace(p, 0x52474220);
            c::cmsSetPCS(p, 0x4C616220);
            c::cmsSetProfileVersion(p, 4.4);
            let pipe = mk_pipe();
            let ok = c::cmsWriteTag(p, tag,
                pipe as *const _);
            // save + reopen
            let mut buf = [0u8; 1 << 17];
            let mut n = buf.len() as u32;
            let sv = c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            let pr = if sv != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !pr.is_null() {
                c::cmsReadTag(pr, tag)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={ok} sv={sv} rd={}",
                !rd.is_null());
            if !pr.is_null() {
                c::cmsCloseProfile(pr); }
            c::cmsCloseProfile(p);
            c::cmsPipelineFree(pipe);
        }
        c::cmsCloseProfile(lab4);
    }
}
