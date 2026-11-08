//! cmsio1 remaining surface: header flag/attr/creator/model/ID getters
//! across the corpus, plus _cmsReadDevicelinkLUT pipeline eval16 parity.

use lcms::{io1, profile};
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
fn header_getter_matrix() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            assert!(!h.is_null());
            let mut cf = 0u32;
            c::cmsGetHeaderFlags(h, &mut cf);
            assert_eq!(cf, rp.flags, "flags");
            let mut ca = 0u64;
            c::cmsGetHeaderAttributes(h, &mut ca);
            assert_eq!(ca, rp.attributes, "attributes");
            assert_eq!(c::cmsGetHeaderCreator(h), rp.creator, "creator");
            assert_eq!(c::cmsGetHeaderModel(h), rp.model, "model");
            let mut cid = [0u8; 16];
            c::cmsGetHeaderProfileID(h, cid.as_mut_ptr());
            assert_eq!(cid, rp.profile_id.id8, "profile_id");
            c::cmsCloseProfile(h);
        }
    }
}

#[test]
fn devicelink_lut_eval() {
    let mut seed = 0x7777u64;
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        for intent in 0..4u32 {
            let clut = unsafe {
                let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
                let l = c::_cmsReadDevicelinkLUT(h, intent);
                c::cmsCloseProfile(h);
                l
            };
            let rlut = io1::read_devicelink_lut(&mut rp, intent);
            assert_eq!(clut.is_null(), rlut.is_none(), "dlut presence i{intent}");
            if clut.is_null() || rlut.is_none() { continue; }
            let rlut = rlut.unwrap();
            let n_in = rlut.input_channels as usize;
            let n_out = rlut.output_channels as usize;
            for _ in 0..8 {
                let inp: Vec<u16> = (0..n_in).map(|_| {
                    seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                    (seed >> 48) as u16
                }).collect();
                let mut co = vec![0u16; n_out];
                let mut ro = vec![0u16; n_out];
                unsafe { c::cmsPipelineEval16(inp.as_ptr(), co.as_mut_ptr(), clut) };
                rlut.eval16(&inp, &mut ro);
                assert_eq!(co, ro, "dlut eval i{intent} {inp:?}");
            }
            unsafe { c::cmsPipelineFree(clut) };
        }
    }
}
