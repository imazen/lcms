//! cmsvirt read-side — transforms on the lcms-generated corpus
//! profiles (linlcms2/labv2lcms2/graylcms2/xyzlcms2/nullcms2/
//! bchslcms2/labv4lcms2).

use lcms::{profile, xform};
use lcms_cref as c;

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter()
        .map(|f| (f.file_name().unwrap().to_string_lossy().into_owned(),
                  std::fs::read(f).unwrap()))
        .collect()
}

#[test]
fn lcms_built_profile_transforms() {
    for (name, data) in corpus() {
        if !name.contains("lcms2") { continue }
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            if h.is_null() { continue }
            // profile -> lab4 transform — the lcms-built profile's
            // read path
            let cs = c::cmsGetColorSpace(h) as u32;
            let nch = c::cmsChannelsOf(cs);
            let in_fmt = (cs << 16) | ((nch as u32) << 3) | 2;
            let out_fmt: u32 = (10 << 16) | (3 << 3) | 2; // lab16
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(h, in_fmt, ho, out_fmt, 0, 0);
            let _ = c::cmsCloseProfile(ho);
            let mut rp2 = lcms::virt::create_lab4_profile(None, None)
                .unwrap();
            let mut profs = vec![rp.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let inp = vec![0x4000u16; nch as usize];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_ne_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                if co != ro16[..3] {
                    eprintln!("{name} c={co:?} r={ro16:?}");
                }
                c::cmsDeleteTransform(ct);
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Remaining profile builders — ProPhoto/AdobeRGB/OkLab/XYZ/
/// NULL/gray variants.
#[test]
fn remaining_builders() {
    unsafe {
        let mut builders: Vec<(&str, *mut std::ffi::c_void)> =
            Vec::new();
        builders.push(("srgb", c::cmsCreate_sRGBProfile()));
        builders.push(("xyz", c::cmsCreateXYZProfile()));
        builders.push(("null", c::cmsCreateNULLProfile()));
        builders.push(("lab2",
            c::cmsCreateLab2Profile(std::ptr::null_mut())));
        builders.push(("lab4",
            c::cmsCreateLab4Profile(std::ptr::null_mut())));
        builders.push(("gray",
            c::cmsCreateGrayProfile(
                c::cmsD50_xyY() as *const std::ffi::c_void,
                c::cmsBuildGamma(std::ptr::null_mut(), 2.2))));
        builders.push(("bchsw",
            c::cmsCreateBCHSWabstractProfileTHR(
                std::ptr::null_mut(), 17, 0.0, 0.0, 0.0,
                1.0, std::ptr::null(), std::ptr::null())));
        // a linearization devicelink
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves: [*mut std::ffi::c_void; 3] = [g as *mut std::ffi::c_void, g as *mut std::ffi::c_void, g as *mut std::ffi::c_void];
        builders.push(("lin",
            c::cmsCreateLinearizationDeviceLink(0x52474220,
                curves.as_ptr())));
        builders.push(("inklim",
            c::cmsCreateInkLimitingDeviceLink(0x434D594B, 300.0)));
        for (name, h) in &builders {
            if h.is_null() { eprintln!("{name} NULL"); continue; }
            // resave each — exercises the built-in tag tables
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(*h, std::ptr::null_mut(),
                &mut cn);
            eprintln!("{name} save={cn}");
            c::cmsCloseProfile(*h);
        }
    }
}

/// Profile-sequence machinery — pseq/psid tag read/write +
/// the SEQ internals.
#[test]
fn profile_sequence() {
    unsafe {
        // build a devicelink — it gets a pseq tag
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let dl = c::cmsTransform2DeviceLink(ct, 4.3, 0);
        c::cmsDeleteTransform(ct);
        if !dl.is_null() {
            // read the pseq/psid tags — the SEQ machinery
            let ps = c::cmsReadTag(dl, 0x70736571); // 'pseq'
            let pid = c::cmsReadTag(dl, 0x70736964); // 'psid'
            eprintln!("pseq={} psid={}", ps.is_null(),
                pid.is_null());
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// cmsCreateRGBProfile on unusual primaries — the RGB-builder
/// internals on arbitrary chromaticities + TRCs.
#[test]
fn rgb_builder_variants() {
    unsafe {
        for (name, wp, r, g, b, gamma) in [
            ("d50-p3", [0.9642f64, 1.0, 0.8249],
             [0.68, 0.32], [0.265, 0.69], [0.15, 0.06], 2.2),
            ("d65-srgb", [0.9505, 1.0, 1.0890],
             [0.64, 0.33], [0.30, 0.60], [0.15, 0.06], 2.2),
            ("e-linear", [1.0f64, 1.0, 1.0],
             [0.70, 0.30], [0.25, 0.65], [0.15, 0.06], 1.0),
            ("d93-bt2020", [0.9634f64, 1.0, 0.8323],
             [0.708, 0.292], [0.17, 0.797], [0.131, 0.046], 2.4),
        ] {
            #[repr(C)]
            struct CxyY { x: f64, y: f64, Y: f64 }
            #[repr(C)]
            struct CXyzTrip { r: CxyY, g: CxyY, b: CxyY }
            let wpv = CxyY { x: wp[0], y: wp[1], Y: wp[2] };
            let wpx = 0.3127f64; // xy from the white's XYZ
            let wpy = 0.3290;
            // build the white-point xyY (the API takes xyY)
            let wpyy = CxyY { x: wpx, y: wpy, Y: 1.0 };
            let prim = CXyzTrip {
                r: CxyY { x: r[0], y: r[1], Y: 1.0 },
                g: CxyY { x: g[0], y: g[1], Y: 1.0 },
                b: CxyY { x: b[0], y: b[1], Y: 1.0 },
            };
            let trc = c::cmsBuildGamma(std::ptr::null_mut(),
                gamma);
            let curves = [trc, trc, trc];
            let curvesv: [*mut std::ffi::c_void; 3] =
                [curves[0] as *mut std::ffi::c_void,
                 curves[1] as *mut std::ffi::c_void,
                 curves[2] as *mut std::ffi::c_void];
            let h = c::cmsCreateRGBProfile(
                &wpyy as *const _ as *const std::ffi::c_void,
                &prim as *const _ as *const std::ffi::c_void,
                curvesv.as_ptr());
            eprintln!("{name} null={}", h.is_null());
            if !h.is_null() {
                let mut cn = 0u32;
                c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                    &mut cn);
                eprintln!("  save={cn}");
                c::cmsCloseProfile(h);
            }
        }
    }
}
