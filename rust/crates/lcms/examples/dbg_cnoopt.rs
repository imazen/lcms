use lcms_cref as c;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const NOOPT: u32 = 0x0040; // cmsFLAGS_NOOPTIMIZE
fn main() {
    unsafe {
        let cs = std::ffi::CString::new(PATH).unwrap();
        let acc = std::ffi::CString::new("r").unwrap();
        let p = c::cmsOpenProfileFromFile(cs.as_ptr(), acc.as_ptr() as *const u8);
        let srgb = c::cmsCreate_sRGBProfile();
        for (nm, fl) in [("opt", 0), ("noopt", NOOPT)] {
            let t = c::cmsCreateTransform(p, TYPE_RGB_8, srgb, TYPE_RGB_8, 0, fl);
            let mut out = [0u8; 3];
            let inp = [0u8; 3];
            c::cmsDoTransform(t, inp.as_ptr() as *const _, out.as_mut_ptr() as *mut _, 1);
            println!("C {nm} at 0: {:?}", out);
        }
        c::cmsCloseProfile(p);
        c::cmsCloseProfile(srgb);
    }
}
