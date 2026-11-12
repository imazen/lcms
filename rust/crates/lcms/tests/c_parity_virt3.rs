//! cmsvirt device-link builders: linearization links on Gray/RGB/Lab/
//! CMYK/MCH5-8, ink-limiting on CMYK at several limits — byte-exact
//! serialized profiles (created masked).

use lcms::{gamma, profile, virt};
use lcms_cref as c;

const CREATED_OFF: usize = 24;

fn c_save(h: *mut std::ffi::c_void) -> Vec<u8> {
    unsafe {
        let mut buf = vec![0u8; 1 << 20];
        let mut size = buf.len() as u32;
        let ok = c::cmsSaveProfileToMem(h, buf.as_mut_ptr() as *mut _, &mut size);
        c::cmsCloseProfile(h);
        if ok == 0 {
            return vec![];
        }
        buf.truncate(size as usize);
        buf
    }
}

fn r_save(p: &mut lcms::profile::IccProfile) -> Vec<u8> {
    profile::save_profile_to_mem(p).unwrap_or_default()
}

fn mask_created(b: &mut Vec<u8>) {
    for i in 0..12 {
        b.get_mut(CREATED_OFF + i).map(|x| *x = 0);
    }
}

fn cs_sig(space: u32) -> u32 {
    space
}

#[test]
fn linearization_links() {
    // color-space sigs: Gray=0x47524159? No — use the lcms signature
    // values directly (cs_sh color-space field is the packed sig).
    let spaces: Vec<u32> = vec![
        0x47524159, // 'GRAY'
        0x52474220, // 'RGB '
        0x4C616220, // 'Lab '
        0x434D594B, // 'CMYK'
        0x4D434835, // 'MCH5'
        0x4D434836, // 'MCH6'
        0x4D434837, // 'MCH7'
        0x4D434838, // 'MCH8'
    ];
    let r_gamma = gamma::build_gamma(None, 2.2).unwrap();
    for &sp in &spaces {
        let n_ch = lcms::pcs::channels_of_color_space(sp);
        if n_ch <= 0 {
            continue;
        }
        let c_g = unsafe { c::cmsBuildGamma(std::ptr::null_mut(), 2.2) };
        let curves = vec![c_g; n_ch as usize];
        let ch = unsafe {
            c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                cs_sig(sp),
                curves.as_ptr(),
            )
        };
        let rtrs: Vec<&gamma::ToneCurve> = vec![&r_gamma; n_ch as usize];
        let rp = virt::create_linearization_device_link(None, cs_sig(sp), &rtrs);
        assert_eq!(ch.is_null(), rp.is_none(), "lin {sp:#x} presence");
        if ch.is_null() || rp.is_none() {
            unsafe {
                if !c_g.is_null() {
                    c::cmsFreeToneCurve(c_g)
                }
            };
            continue;
        }
        let mut c_out = c_save(ch);
        let mut r_out = r_save(&mut rp.unwrap());
        mask_created(&mut c_out);
        mask_created(&mut r_out);
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "lin {sp:#x}");
        unsafe { c::cmsFreeToneCurve(c_g) };
    }
}

#[test]
fn ink_limiting() {
    for limit in [50.0f64, 75.0, 90.0, 99.0, 100.0, 150.0] {
        let ch = unsafe {
            c::cmsCreateInkLimitingDeviceLinkTHR(std::ptr::null_mut(), 0x434D594B, limit)
        };
        let rp = virt::create_ink_limiting_device_link(None, 0x434D594B, limit);
        assert_eq!(ch.is_null(), rp.is_none(), "ink {limit} presence");
        if ch.is_null() || rp.is_none() {
            continue;
        }
        let mut c_out = c_save(ch);
        let mut r_out = r_save(&mut rp.unwrap());
        mask_created(&mut c_out);
        mask_created(&mut r_out);
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "ink {limit}");
    }
}

#[test]
fn gray_xyz_profiles() {
    // Gray profiles with different whitepoints/curves
    let wps: Vec<Option<lcms::types::CiexyY>> = vec![
        None,
        Some(lcms::types::CiexyY {
            x: 0.3127,
            y: 0.3290,
            Y: 1.0,
        }), // D65
        Some(lcms::types::CiexyY {
            x: 0.3457,
            y: 0.3585,
            Y: 1.0,
        }), // D50
    ];
    for (i, wp) in wps.iter().enumerate() {
        let r_gamma = gamma::build_gamma(None, 1.8 + i as f64 * 0.2).unwrap();
        let c_g = unsafe { c::cmsBuildGamma(std::ptr::null_mut(), 1.8 + i as f64 * 0.2) };
        let cwp = wp.map(|w| c::CiexyY {
            x: w.x,
            y: w.y,
            Y: w.Y,
        });
        let ch = unsafe {
            c::cmsCreateGrayProfile(
                cwp.as_ref()
                    .map(|w| w as *const _ as *const std::ffi::c_void)
                    .unwrap_or(std::ptr::null()),
                c_g,
            )
        };
        let rp = virt::create_gray_profile(None, wp.as_ref(), Some(&r_gamma));
        assert_eq!(ch.is_null(), rp.is_none(), "gray {i} presence");
        if ch.is_null() || rp.is_none() {
            unsafe { c::cmsFreeToneCurve(c_g) };
            continue;
        }
        let mut c_out = c_save(ch);
        let mut r_out = r_save(&mut rp.unwrap());
        mask_created(&mut c_out);
        mask_created(&mut r_out);
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "gray {i}");
        unsafe { c::cmsFreeToneCurve(c_g) };
    }
}
