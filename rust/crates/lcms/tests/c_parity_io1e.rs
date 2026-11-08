//! cmsio1 intent-support matrix: cmsIsIntentSupported across the
//! corpus x intents 0-255 x input/output directions.

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
fn intent_support_matrix() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            for intent in 0..8u32 {
                for dir in [1u32, 2] { // LCMS_USED_AS_INPUT/OUTPUT
                    let c_ok = c::cmsIsIntentSupported(h, intent, dir);
                    let r_ok = io1::is_intent_supported(&rp, intent, dir as i32);
                    assert_eq!(c_ok != 0, r_ok,
                        "intent {intent} dir {dir}");
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}
