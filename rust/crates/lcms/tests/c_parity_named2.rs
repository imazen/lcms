//! cmsnamed edge cases: empty list, boundary indices, index misses,
//! large lists, dup — named-color list machinery beyond the basic
//! get/set coverage.

use lcms::named;
use lcms_cref as c;

fn cs8(s: &str) -> *const u8 {
    // CMS APIs expect NUL-terminated C strings
    thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        b.as_ptr()
    })
}

#[test]
fn named_color_edge_cases() {
    unsafe {
        // empty list — count=0, index miss, info on empty
        let cl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 0, 3, cs8("RGB ").cast(), cs8("RGB ").cast());
        let rl = named::alloc_named_color_list(None, 0, 3, b"RGB ", b"RGB ");
        assert_eq!(cl.is_null(), rl.is_none(), "empty alloc");
        if let Some(rl) = rl {
            assert_eq!(
                c::cmsNamedColorCount(cl),
                named::named_color_count(&rl),
                "empty count"
            );
            assert_eq!(
                c::cmsNamedColorIndex(cl, cs8("nope").cast()),
                named::named_color_index(&rl, b"nope"),
                "empty index miss"
            );
            let mut cn = [0u8; 32];
            let mut cpfx = [0u8; 33];
            let mut csfx = [0u8; 33];
            let mut cpcs = [0u16; 3];
            let mut cc = [0u16; 16];
            let cinfo = c::cmsNamedColorInfo(
                cl,
                0,
                cn.as_mut_ptr().cast(),
                cpfx.as_mut_ptr().cast(),
                csfx.as_mut_ptr().cast(),
                cpcs.as_mut_ptr(),
                cc.as_mut_ptr(),
            );
            let rinfo = named::named_color_info(&rl, 0);
            assert_eq!(cinfo != 0, rinfo.is_some(), "empty info");
            c::cmsFreeNamedColorList(cl);
        }
        // large list — 1000 entries, boundary index lookups
        let cl2 = c::cmsAllocNamedColorList(std::ptr::null_mut(), 0, 3, cs8("RGB ").cast(), cs8("RGB ").cast());
        let mut rl2 = named::alloc_named_color_list(None, 0, 3, b"RGB ", b"RGB ").unwrap();
        for i in 0..1000 {
            let name = format!("c{i:04}");
            let mut pcs = [0u16; 16];
            pcs[0] = (i % 65536) as u16;
            pcs[1] = ((i * 7) % 65536) as u16;
            pcs[2] = ((i * 13) % 65536) as u16;
            let pcs3 = [pcs[0], pcs[1], pcs[2]];
            assert_eq!(
                c::cmsAppendNamedColor(cl2, cs8(&name).cast(), pcs3.as_ptr(), [0u16; 16].as_ptr()) != 0,
                named::append_named_color(
                    &mut rl2,
                    Some(name.as_bytes()),
                    Some(&pcs3),
                    Some(&[0u16; 16])
                ),
                "append {i}"
            );
        }
        assert_eq!(
            c::cmsNamedColorCount(cl2),
            named::named_color_count(&rl2),
            "count 1000"
        );
        // boundary + miss index parity
        for idx_name in ["c0000", "c0999", "c0500", "nope", "c1000"] {
            assert_eq!(
                c::cmsNamedColorIndex(cl2, cs8(idx_name).cast()),
                named::named_color_index(&rl2, idx_name.as_bytes()),
                "index {idx_name}"
            );
        }
        // boundary info
        for i in [0u32, 500, 999, 1000] {
            let mut cn = [0u8; 32];
            let mut cpfx = [0u8; 33];
            let mut csfx = [0u8; 33];
            let mut cpcs = [0u16; 3];
            let mut cc = [0u16; 16];
            let cinfo = c::cmsNamedColorInfo(
                cl2,
                i,
                cn.as_mut_ptr().cast(),
                cpfx.as_mut_ptr().cast(),
                csfx.as_mut_ptr().cast(),
                cpcs.as_mut_ptr(),
                cc.as_mut_ptr(),
            );
            let rinfo = named::named_color_info(&rl2, i);
            assert_eq!(cinfo != 0, rinfo.is_some(), "info {i}");
            if cinfo != 0 {
                let cs = unsafe { std::ffi::CStr::from_ptr(cn.as_ptr() as *const std::ffi::c_char) };
                let (rn, _rp, _rs, rpcs, _rc) = rinfo.unwrap();
                let rend = rn.iter().position(|&b| b == 0).unwrap_or(rn.len());
                assert_eq!(cs.to_bytes().to_vec(), rn[..rend].to_vec(), "info name {i}");
                assert_eq!(cpcs[..3], rpcs[..3], "info pcs {i}");
            }
        }
        // dup parity
        let cd = c::cmsDupNamedColorList(cl2);
        let rd = named::dup_named_color_list(&rl2).unwrap();
        assert_eq!(
            c::cmsNamedColorCount(cd),
            named::named_color_count(&rd),
            "dup count"
        );
        c::cmsFreeNamedColorList(cl2);
        c::cmsFreeNamedColorList(cd);
    }
}
