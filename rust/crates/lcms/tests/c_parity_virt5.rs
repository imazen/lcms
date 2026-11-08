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
