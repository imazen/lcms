use lcms_cref as c;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
#[repr(C)] struct CXyz { x: f64, y: f64, z: f64 }
fn main() {
    unsafe {
        let cs = std::ffi::CString::new(PATH).unwrap();
        let acc = std::ffi::CString::new("r").unwrap();
        let p = c::cmsOpenProfileFromFile(cs.as_ptr(), acc.as_ptr() as *const u8);
        for (name, sig) in [("rXYZ", 0x7258595a), ("gXYZ", 0x6758595a), ("bXYZ", 0x6258595a), ("wtpt", 0x77747074)] {
            let v = c::cmsReadTag(p, sig) as *const CXyz;
            if !v.is_null() {
                let x = &*v;
                println!("C {name} = {:.6} {:.6} {:.6}", x.x, x.y, x.z);
            }
        }
        c::cmsCloseProfile(p);
    }
}
