use lcms_cref as c;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn main() {
    unsafe {
        let cs = std::ffi::CString::new(PATH).unwrap();
        let acc = std::ffi::CString::new("r").unwrap();
        let p = c::cmsOpenProfileFromFile(cs.as_ptr(), (acc.as_ptr() as *const u8).cast());
        assert!(!p.is_null());
        // rTRC sig = 'rTRC' = 0x72545243
        let trc = c::cmsReadTag(p, 0x72545243);
        assert!(!trc.is_null());
        for v in [0u16, 256, 1024, 0x4000, 0xffff] {
            let e = c::cmsEvalToneCurve16(trc as *const _, v);
            println!("C rTRC[{:04x}] = {:04x}", v, e);
        }
        c::cmsCloseProfile(p);
    }
}
