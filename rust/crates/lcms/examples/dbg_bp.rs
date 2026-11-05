use lcms::profile;
use lcms::sm;
use lcms::virt;
use lcms_cref as c;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
#[repr(C)]
#[derive(Debug)]
struct CXyz {
    x: f64,
    y: f64,
    z: f64,
}
fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").unwrap();
    let bp = sm::detect_black_point(&mut p, 0, 0);
    println!("rust ibm-t61 bp: {:?}", bp);
    let mut s = virt::create_srgb_profile(None).unwrap();
    let dbp = sm::detect_destination_black_point(&mut s, 0, 0);
    println!("rust sRGB dbp: {:?}", dbp);
    unsafe {
        let cs = std::ffi::CString::new(PATH).unwrap();
        let acc = std::ffi::CString::new("r").unwrap();
        let ip = c::cmsOpenProfileFromFile(cs.as_ptr(), acc.as_ptr() as *const u8);
        let mut o = CXyz {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let r = c::cmsDetectBlackPoint(&mut o as *mut _ as *mut _, ip, 0, 0);
        println!("C ibm-t61 bp: r={} {:?}", r, o);
        let srgb = c::cmsCreate_sRGBProfile();
        let mut o2 = CXyz {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let r2 = c::cmsDetectDestinationBlackPoint(&mut o2 as *mut _ as *mut _, srgb, 0, 0);
        println!("C sRGB dbp: r={} {:?}", r2, o2);
        c::cmsCloseProfile(ip);
        c::cmsCloseProfile(srgb);
    }
}
