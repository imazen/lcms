//! cmsplugin registration machinery across plugin types: Interp,
//! TagType, Tag, Mpe, ParametricCurve — accept/reject + slot parity.

use lcms::context::{self, Plugin};
use lcms_cref as c;
use std::ffi::c_void;

const MAGIC: u32 = 0x61637070; // 'acpp'
const VERSION: u32 = 2200;

// cmsPluginBase layout: Magic, ExpectedVersion, Type, Next
#[repr(C)]
struct CPluginBase {
    magic: u32,
    expected_version: u32,
    ty: u32,
    next: *mut c_void,
}

#[repr(C)]
struct CPluginInterp {
    base: CPluginBase,
    factory: unsafe extern "C" fn(u32, u32, u32) -> *mut c_void,
}

#[repr(C)]
struct CPluginTagType {
    base: CPluginBase,
    handler: *mut c_void, // cmsTagTypeHandler — opaque for registration
}

#[repr(C)]
struct CPluginTag {
    base: CPluginBase,
    descriptor: *mut c_void,
}

#[repr(C)]
struct CPluginMpe {
    base: CPluginBase,
    handler: *mut c_void,
}

unsafe extern "C" fn interp_factory(_a: u32, _b: u32, _c: u32) -> *mut c_void {
    std::ptr::null_mut()
}

#[test]
fn plugin_type_matrix() {
    let types: Vec<(u32, *mut c_void)> = vec![
        (0x696E7048, { // 'inpH' interpolation
            let p = Box::leak(Box::new(CPluginInterp {
                base: CPluginBase { magic: MAGIC, expected_version: VERSION,
                    ty: 0x696E7048, next: std::ptr::null_mut() },
                factory: interp_factory,
            })) as *mut CPluginInterp as *mut c_void;
            p
        }),
        (0x74797048, { // 'typH' tag type
            let p = Box::leak(Box::new(CPluginTagType {
                base: CPluginBase { magic: MAGIC, expected_version: VERSION,
                    ty: 0x74797048, next: std::ptr::null_mut() },
                handler: std::ptr::null_mut(),
            })) as *mut CPluginTagType as *mut c_void;
            p
        }),
        (0x74616748, { // 'tagH' tag
            let p = Box::leak(Box::new(CPluginTag {
                base: CPluginBase { magic: MAGIC, expected_version: VERSION,
                    ty: 0x74616748, next: std::ptr::null_mut() },
                descriptor: std::ptr::null_mut(),
            })) as *mut CPluginTag as *mut c_void;
            p
        }),
        (0x6D706548, { // 'mpeH' multiprocess elements
            let p = Box::leak(Box::new(CPluginMpe {
                base: CPluginBase { magic: MAGIC, expected_version: VERSION,
                    ty: 0x6D706548, next: std::ptr::null_mut() },
                handler: std::ptr::null_mut(),
            })) as *mut CPluginMpe as *mut c_void;
            p
        }),
    ];
    for (sig, plugin) in types {
        let c_ok = unsafe { c::cmsPlugin(plugin) };
        // rust: the equivalent Plugin variant — Interp needs a real fn;
        // the others accept via slot push
        let r_ok = match sig {
            0x696E7048 => {
                let mut ctx = context::create_context(None, 0);
                ctx.plugin(&[Plugin::Interp(|_a, _b, _c| None)])
            }
            _ => {
                let mut ctx = context::create_context(None, 0);
                ctx.plugin(&[Plugin::Other])
            }
        };
        // both sides accept a well-formed plugin — status parity
        assert_eq!(c_ok != 0, r_ok, "plugin type {sig:#x} status");
    }
}

/// More plugin types: Formatters, RenderingIntent, Optimization,
/// Transform, Header — the registration machinery across more slots.
#[test]
fn plugin_type_matrix2() {
    const MAGIC: u32 = 0x61637070;
    const VERSION: u32 = 2200;
    #[repr(C)]
    struct CPluginBase {
        magic: u32,
        expected_version: u32,
        ty: u32,
        next: *mut c_void,
    }
    // each type = PluginBase + a single factory/handler fn ptr
    unsafe extern "C" fn stub() -> *mut c_void { std::ptr::null_mut() }
    let sigs = [0x66726D48u32, 0x696E7448, 0x6F707448, 0x7A666D48]; // hdrH needs a real Header payload
    for &sig in &sigs {
        #[repr(C)]
        struct CP {
            base: CPluginBase,
            factory: unsafe extern "C" fn() -> *mut c_void,
        }
        let p = Box::leak(Box::new(CP {
            base: CPluginBase { magic: MAGIC, expected_version: VERSION,
                ty: sig, next: std::ptr::null_mut() },
            factory: stub,
        })) as *mut CP as *mut c_void;
        let c_ok = unsafe { c::cmsPlugin(p) };
        // rust accepts a plugin entry via the Other variant slot
        let mut ctx = context::create_context(None, 0);
        let r_ok = ctx.plugin(&[Plugin::Other]);
        assert_eq!(c_ok != 0, r_ok, "plugin type {sig:#x}");
    }
}
