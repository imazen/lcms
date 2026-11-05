//! Named-color transform parity: `TYPE_NAMED_COLOR_INDEX` → Lab16 via a
//! namedColor2-class profile. The profile bytes are built once by C (the
//! known-good writer), then opened in both implementations. The gate
//! compares transform output bytes plus the named-color list accessors
//! (`cmsGetNamedColorList` / `*TransformInputColorants`).

use lcms::virt;
use lcms::xform;
use lcms_cref as c;
use std::ffi::CString;

const TYPE_NAMED_COLOR_INDEX: u32 = 1 << 3 | 2;
const TYPE_LAB_16: u32 = 10 << 16 | 3 << 3 | 2;
const SIG_NAMED_COLOR_CLASS: u32 = 0x6e6d636c; // 'nmcl'
const SIG_CMYK: u32 = 0x434d594b; // 'CMYK'
const SIG_LAB: u32 = 0x4c616220; // 'Lab '
const SIG_NCL2: u32 = 0x6e636c32; // 'ncl2'
const SIG_CLRT: u32 = 0x636c7274; // 'clrt'
const SIG_WTPT: u32 = 0x77747074; // 'wtpt'

/// Build a namedColor2-class profile in C, save to memory.
/// Also writes a `clrt` colorant-table tag so `InputColorant` is populated.
fn c_named_profile_bytes(
    names: &[&str],
    labs: &[(f64, f64, f64)],
    colorants: &[[u16; 4]],
) -> Vec<u8> {
    unsafe {
        let p = c::cmsCreateNULLProfile();
        assert!(!p.is_null(), "cmsCreateNULLProfile failed");
        c::cmsSetProfileVersion(p, 4.3);
        c::cmsSetDeviceClass(p, SIG_NAMED_COLOR_CLASS);
        c::cmsSetColorSpace(p, SIG_CMYK);
        c::cmsSetPCS(p, SIG_LAB);
        c::cmsSetHeaderRenderingIntent(p, 0);
        assert_eq!(c::cmsWriteTag(p, SIG_WTPT, c::cmsD50_XYZ() as *const _), 1);

        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(),
            names.len() as u32,
            4,
            c"TEST".as_ptr() as *const _,
            c"SUF".as_ptr() as *const _,
        );
        assert!(!list.is_null(), "cmsAllocNamedColorList failed");
        for ((name, (l, a, b)), col) in names.iter().zip(labs.iter()).zip(colorants.iter()) {
            let name_c = CString::new(*name).unwrap();
            let lab = c::CieLab {
                L: *l,
                a: *a,
                b: *b,
            };
            let mut pcs = [0u16; 3];
            c::cmsFloat2LabEncodedV2(pcs.as_mut_ptr(), &lab);
            let mut dev = [0u16; 16];
            dev[..4].copy_from_slice(col);
            assert_eq!(
                c::cmsAppendNamedColor(list, name_c.as_ptr() as *const _, pcs.as_ptr(), dev.as_ptr()),
                1
            );
        }
        assert_eq!(c::cmsWriteTag(p, SIG_NCL2, list as *const _), 1);
        // a colorant-table tag feeds _cmsTRANSFORM::InputColorant
        assert_eq!(c::cmsWriteTag(p, SIG_CLRT, list as *const _), 1);

        let mut size = 0u32;
        assert_eq!(c::cmsSaveProfileToMem(p, std::ptr::null_mut(), &mut size), 1);
        let mut buf = vec![0u8; size as usize];
        assert_eq!(
            c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut size),
            1
        );
        buf.truncate(size as usize);
        c::cmsFreeNamedColorList(list);
        c::cmsCloseProfile(p);
        buf
    }
}

fn names() -> Vec<&'static str> {
    vec![
        "Hazelnut 14-1315",
        "Kale 18-0107",
        "PANTONE 100 C",
        "PANTONE 200 C",
        "PANTONE Reflex Blue",
    ]
}

fn labs() -> Vec<(f64, f64, f64)> {
    vec![
        (50.0, 10.0, -10.0),
        (40.0, -5.0, 8.0),
        (88.0, -3.0, 90.0),
        (30.0, 60.0, -20.0),
        (20.0, 10.0, -70.0),
    ]
}

fn colorants() -> Vec<[u16; 4]> {
    vec![
        [10 * 257, 20 * 257, 30 * 257, 40 * 257],
        [10 * 257, 20 * 257, 30 * 257, 40 * 257],
        [0, 0, 60 * 257, 0],
        [20 * 257, 90 * 257, 30 * 257, 10 * 257],
        [90 * 257, 70 * 257, 0, 20 * 257],
    ]
}

#[test]
fn named_color_index_to_lab() {
    let bytes = c_named_profile_bytes(&names(), &labs(), &colorants());
    let n = names().len() + 1; // one out-of-range index at the tail
    let inp: Vec<u8> = (0..n as u16).flat_map(|v| v.to_le_bytes()).collect();

    // ---- C side ----
    let mut c_out = vec![0u8; n * 6];
    let mut c_ncolors = 0u32;
    let mut c_clrt_count = 0u32;
    let mut c_info: Vec<(String, [u16; 3], [u16; 4])> = Vec::new();
    unsafe {
        let hn = c::cmsOpenProfileFromMem(bytes.as_ptr() as *const _, bytes.len() as u32);
        assert!(!hn.is_null(), "C reopen failed");
        let hl = c::cmsCreateLab4Profile(std::ptr::null());
        assert!(!hl.is_null());
        let t = c::cmsCreateTransform(hn, TYPE_NAMED_COLOR_INDEX, hl, TYPE_LAB_16, 0, 0);
        assert!(!t.is_null(), "C named-color transform failed");
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            n as u32,
        );
        let glist = c::cmsGetNamedColorList(t);
        if !glist.is_null() {
            c_ncolors = c::cmsNamedColorCount(glist);
            for i in 0..c_ncolors {
                let mut name = [0u8; 256];
                let mut pcs = [0u16; 3];
                let mut col = [0u16; 16];
                assert_eq!(
                    c::cmsNamedColorInfo(
                        glist,
                        i,
                        name.as_mut_ptr(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        pcs.as_mut_ptr(),
                        col.as_mut_ptr(),
                    ),
                    1
                );
                let nend = name.iter().position(|&b| b == 0).unwrap_or(256);
                c_info.push((
                    String::from_utf8_lossy(&name[..nend]).into_owned(),
                    pcs,
                    [col[0], col[1], col[2], col[3]],
                ));
            }
        }
        let ilist = c::cmsGetTransformInputColorants(t);
        if !ilist.is_null() {
            c_clrt_count = c::cmsNamedColorCount(ilist);
        }
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(hn);
        c::cmsCloseProfile(hl);
    }

    // ---- Rust side ----
    let mut r_out = vec![0u8; n * 6];
    let p_named = lcms::profile::open_profile_from_mem(None, &bytes).expect("rust reopen failed");
    let p_lab = virt::create_lab4_profile(None, None).expect("lab4");
    let mut profs = vec![p_named, p_lab];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_NAMED_COLOR_INDEX,
        TYPE_LAB_16,
        0,
    )
    .expect("rust named-color transform failed");

    // accessor parity BEFORE eval
    let rlist = xform::get_named_color_list(&t).expect("rust get_named_color_list");
    assert_eq!(rlist.n_colors, c_ncolors, "named-color count mismatch");
    let r_in = xform::get_transform_input_colorants(&t);
    assert_eq!(
        r_in.map(|l| l.n_colors).unwrap_or(0),
        c_clrt_count,
        "input colorant table count mismatch"
    );
    assert!(xform::get_transform_output_colorants(&t).is_none());
    for (i, (cname, cpcs, ccol)) in c_info.iter().enumerate() {
        let e = &rlist.list[i];
        let nend = e.name.iter().position(|&b| b == 0).unwrap_or(e.name.len());
        assert_eq!(&String::from_utf8_lossy(&e.name[..nend]), cname, "name[{i}]");
        assert_eq!(&e.pcs, cpcs, "pcs[{i}]");
        assert_eq!(&e.device_colorant[..4], ccol, "colorant[{i}]");
    }

    t.do_transform(&inp, &mut r_out, n as u32);
    assert_eq!(c_out, r_out, "named-color index→Lab16 output mismatch");
}
