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
