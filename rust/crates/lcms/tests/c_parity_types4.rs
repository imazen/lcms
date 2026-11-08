//! cmstypes write-side tag handlers: a battery of exotic tag types
//! written to profiles, resaved, reopened, read back — the per-type
//! writer/reader machinery beyond viewing-conditions.

use lcms::{gamma, named, profile, tagtypes::{TagObject, *}, types::*, virt};
use lcms_cref as c;

/// cmsDateTimeNumber — 6 u32 fields
#[repr(C)]
struct CDateTimeNumber { year: u32, month: u32, day: u32,
    hours: u32, minutes: u32, seconds: u32 }

fn resave_mask(b: &mut Vec<u8>) {
    for i in 0..12 { b.get_mut(24 + i).map(|x| *x = 0); }
    for i in 0..16 { b.get_mut(84 + i).map(|x| *x = 0); }
}

fn write_resave(tag: u32, c_data: *const std::ffi::c_void, r_obj: TagObject,
                name: &str) {
    unsafe {
        let ch = c::cmsCreateXYZProfile();
        let mut rp = virt::create_xyz_profile(None).unwrap();
        let c_ok = c::cmsWriteTag(ch, tag, c_data);
        let r_ok = profile::cms_write_tag(&mut rp, tag, Some(r_obj));
        assert_eq!(c_ok != 0, r_ok, "{name} write");
        if c_ok == 0 { c::cmsCloseProfile(ch); return; }
        // resave both, compare bytes
        let mut c_out = vec![0u8; 1 << 20];
        let mut size = c_out.len() as u32;
        c::cmsSaveProfileToMem(ch, c_out.as_mut_ptr() as *mut _, &mut size);
        c_out.truncate(size as usize);
        let mut r_out = profile::save_profile_to_mem(&mut rp).unwrap_or_default();
        resave_mask(&mut c_out);
        resave_mask(&mut r_out);
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "{name} resave");
        // read-back presence on the reopened bytes
        let ch2 = c::cmsOpenProfileFromMem(c_out.as_ptr() as _, c_out.len() as u32);
        if !ch2.is_null() {
            assert!(!c::cmsReadTag(ch2, tag).is_null(), "{name} read");
            c::cmsCloseProfile(ch2);
        }
        c::cmsCloseProfile(ch);
    }
}

#[test]
fn exotic_writes() {
    unsafe {
        // Curve tag
        let cg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let rg = gamma::build_gamma(None, 2.2).unwrap();
        eprintln!("kTRC write"); write_resave(0x6B747263, cg as *const _, TagObject::Curve(rg), "kTRC");
        // ParametricCurve
        let params: [f64; 10] = { let mut p = [0.0;10]; p[0]=2.4; p[1]=1.0/1.055;
            p[2]=0.055/1.055; p[3]=1.0/12.92; p[4]=0.04045; p };
        let cp = c::cmsBuildParametricToneCurve(std::ptr::null_mut(), 4, params.as_ptr());
        let rp2 = gamma::build_parametric_tone_curve(None, 4, &params).unwrap();
        eprintln!("rTRC-pc write"); write_resave(0x72545952, cp as *const _, TagObject::ParametricCurve(rp2), "rTRC-pc");
        // Signature
        let sig: u32 = 0x61637070;
        eprintln!("tech write"); write_resave(0x74656368, &sig as *const _ as *const _, TagObject::Signature(sig), "tech");
        // DateTime — both sides take  (tm_year = years
        // since 1900) -> 124 encodes 2024
        #[repr(C)]
        struct CTm { tm_sec: i32, tm_min: i32, tm_hour: i32, tm_mday: i32,
            tm_mon: i32, tm_year: i32, tm_wday: i32, tm_yday: i32,
            tm_isdst: i32 }
        let cdt = CTm { tm_sec: 45, tm_min: 30, tm_hour: 10, tm_mday: 15,
            tm_mon: 5, tm_year: 124, tm_wday: 0, tm_yday: 0, tm_isdst: 0 };
        let tm = lcms::tagtypes::Tm { tm_year: 124, tm_mon: 5, tm_mday: 15,
            tm_hour: 10, tm_min: 30, tm_sec: 45, tm_wday: 0, tm_yday: 0,
            tm_isdst: 0 };
        eprintln!("calt-dt write"); write_resave(0x63616C74, &cdt as *const _ as *const _,
            TagObject::DateTime(tm), "calt-dt");
        // XYZ
        let xyz = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        eprintln!("wtpt write"); write_resave(0x77687470, &xyz as *const _ as *const _,
            TagObject::Xyz(CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 }), "wtpt");
        // cg/cp are owned by the profiles after cmsWriteTag — the
        // profile's Close frees them; freeing here would double-free.

        // Chromaticity (chrm) — cmsCIExyYTRIPLE
        #[repr(C)]
        struct CXyy { x: f64, y: f64, Y: f64 }
        #[repr(C)]
        struct CXyyTriple { red: CXyy, green: CXyy, blue: CXyy }
        let cch = CXyyTriple {
            red: CXyy { x: 0.64, y: 0.33, Y: 1.0 },
            green: CXyy { x: 0.30, y: 0.60, Y: 1.0 },
            blue: CXyy { x: 0.15, y: 0.06, Y: 1.0 },
        };
        let rch = CiexyYTriple {
            Red: CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            Green: CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
            Blue: CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        };
        eprintln!("chrm write");
        write_resave(0x6368726D, &cch as *const _ as *const _,
            TagObject::Chromaticity(rch), "chrm");

        // Measurement (meas) — cmsICCMeasurementConditions
        #[repr(C)]
        struct CMeas { observer: u32, backing: c::CieXyz, geometry: u32,
            flare: f64, illuminant_type: u32 }
        let cm = CMeas { observer: 1,
            backing: c::CieXyz { X: 0.89, Y: 1.0, Z: 0.32 },
            geometry: 1, flare: 0.0, illuminant_type: 1 };
        // EncodedXyzNumber is S15Fixed16 (value*65536) — encode the
        // backing f64s the way C's write handler does
        let enc = |v: f64| (v * 65536.0).round() as i32;
        let rm = IccMeasurementConditions { observer: 1,
            backing: EncodedXyzNumber { X: enc(0.89), Y: enc(1.0),
                Z: enc(0.32) },
            geometry: 1, flare: 0.0, illuminant: 1 };
        eprintln!("meas write");
        write_resave(0x6D656173, &cm as *const _ as *const _,
            TagObject::Measurement(rm), "meas");

        // ColorantOrder (clro) — u8[16] index array
        let cco = [0u8, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let rco = [0u8, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        eprintln!("clro write");
        write_resave(0x636C726F, cco.as_ptr() as *const _,
            TagObject::ColorantOrder(rco), "clro");
    }
}
