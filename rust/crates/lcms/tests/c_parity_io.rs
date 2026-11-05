//! Differential test: `lcms::io` vs the cmsio0/cmsplugin IO layer.

use lcms::io::*;
use lcms_cref as c;
use std::ptr;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

#[test]
fn endian_helpers() {
    let mut s = 1u64;
    for _ in 0..10_000 {
        let v16 = rng(&mut s) as u16;
        let v32 = rng(&mut s) as u32;
        let v64 = rng(&mut s);
        unsafe {
            assert_eq!(adjust_endian16(v16), c::_cmsAdjustEndianess16(v16));
            assert_eq!(adjust_endian32(v32), c::_cmsAdjustEndianess32(v32));
            let mut o = 0u64;
            c::_cmsAdjustEndianess64(&mut o, &v64);
            assert_eq!(adjust_endian64(v64), o);
        }
    }
}

#[test]
fn read_write_primitives_roundtrip() {
    // Write a stream of typed values through BOTH impls, compare bytes,
    // then read them back through both and compare values.
    let mut s = 7u64;

    for case in 0..200 {
        // values per case
        let v8 = rng(&mut s) as u8;
        let v16 = rng(&mut s) as u16;
        let v32 = rng(&mut s) as u32;
        let v64 = rng(&mut s);
        let vf = f32::from_bits(rng(&mut s) as u32 % 0x7f800000); // finite range
        let vfx = (rng(&mut s) % 400000) as f64 / 1000.0 - 200.0;
        let xyz = lcms::types::CieXyz {
            X: (rng(&mut s) % 20000) as f64 / 1000.0,
            Y: (rng(&mut s) % 20000) as f64 / 1000.0,
            Z: (rng(&mut s) % 20000) as f64 / 1000.0,
        };
        let sig = rng(&mut s) as u32;

        // C side: NULL handler for write-size accounting then a mem buffer
        let mut cbuf = vec![0u8; 1024];
        let (c_written, c_read_ok) = unsafe {
            let io = c::cmsOpenIOhandlerFromMem(
                ptr::null_mut(),
                cbuf.as_mut_ptr(),
                1024,
                b"w\0".as_ptr(),
            );
            assert!(!io.is_null());
            assert_eq!(c::_cmsWriteUInt8Number(io, v8), 1);
            assert_eq!(c::_cmsWriteUInt16Number(io, v16), 1);
            assert_eq!(c::_cmsWriteUInt32Number(io, v32), 1);
            assert_eq!(c::_cmsWriteUInt64Number(io, &v64), 1);
            assert_eq!(c::_cmsWriteFloat32Number(io, vf), 1);
            assert_eq!(c::_cmsWrite15Fixed16Number(io, vfx), 1);
            assert_eq!(
                c::_cmsWriteXYZNumber(
                    io,
                    &c::CieXyz {
                        X: xyz.X,
                        Y: xyz.Y,
                        Z: xyz.Z
                    }
                ),
                1
            );
            assert_eq!(c::_cmsWriteTypeBase(io, sig), 1);
            assert_eq!(c::_cmsWriteAlignment(io), 1);
            c::cmsCloseIOhandler(io);

            let io = c::cmsOpenIOhandlerFromMem(
                ptr::null_mut(),
                cbuf.as_mut_ptr(),
                1024,
                b"r\0".as_ptr(),
            );
            let (mut a8, mut a16, mut a32, mut a64, mut af, mut afx) =
                (0u8, 0u16, 0u32, 0u64, 0f32, 0f64);
            let mut axyz = c::CieXyz::default();
            c::_cmsReadUInt8Number(io, &mut a8);
            c::_cmsReadUInt16Number(io, &mut a16);
            c::_cmsReadUInt32Number(io, &mut a32);
            c::_cmsReadUInt64Number(io, &mut a64);
            c::_cmsReadFloat32Number(io, &mut af);
            c::_cmsRead15Fixed16Number(io, &mut afx);
            c::_cmsReadXYZNumber(io, &mut axyz);
            let base = c::_cmsReadTypeBase(io);
            let align = c::_cmsReadAlignment(io);
            let ok = (
                a8,
                a16,
                a32,
                a64,
                af.to_bits(),
                afx.to_bits(),
                (axyz.X, axyz.Y, axyz.Z),
                base,
                align,
            );
            c::cmsCloseIOhandler(io);
            (cbuf.len(), ok)
        };

        // Rust side
        let mut rbuf = vec![0u8; 1024];
        {
            let mut io = io_handler_from_mem(None, std::mem::take(&mut rbuf), "w").unwrap();
            assert!(write_u8(&mut io, v8));
            assert!(write_u16(&mut io, v16));
            assert!(write_u32(&mut io, v32));
            assert!(write_u64(&mut io, v64));
            assert!(write_float32(&mut io, vf));
            assert!(write_15_fixed16(&mut io, vfx));
            assert!(write_xyz(&mut io, &xyz));
            assert!(write_type_base(&mut io, sig));
            assert!(write_alignment(&mut io));
            let IoHandler {
                stream: IoStream::Mem(m),
                ..
            } = io
            else {
                panic!()
            };
            rbuf = m.block;
        }
        assert_eq!(&rbuf[..64], &cbuf[..64], "bytes case {case}");

        let mut io = io_handler_from_mem(None, rbuf, "r").unwrap();
        let (mut a8, mut a16, mut a32, mut a64, mut af, mut afx) =
            (0u8, 0u16, 0u32, 0u64, 0f32, 0f64);
        let mut axyz = lcms::types::CieXyz::default();
        assert!(read_u8(&mut io, &mut a8));
        assert!(read_u16(&mut io, &mut a16));
        assert!(read_u32(&mut io, &mut a32));
        assert!(read_u64(&mut io, &mut a64));
        let _ = read_float32(&mut io, &mut af);
        assert!(read_15_fixed16(&mut io, &mut afx));
        assert!(read_xyz(&mut io, &mut axyz));
        let base = read_type_base(&mut io);
        let align = read_alignment(&mut io) as i32;

        assert_eq!(
            (
                a8,
                a16,
                a32,
                a64,
                af.to_bits(),
                afx.to_bits(),
                (axyz.X, axyz.Y, axyz.Z),
                base,
                align
            ),
            c_read_ok,
            "read case {case}"
        );
        let _ = c_written;
    }
}

#[test]
fn float32_read_safeguard() {
    // Absurd/absnormal floats must return FALSE on read — NaN/Inf/subnormal.
    let s = 9u64;
    for bits in [
        0x7f800001u32,
        0x7fc00000,
        0x00000001,
        0x007fffff,
        0x1f7fffff,
        0x7f7fffff,
        0x00800000,
    ] {
        let mut buf = adjust_endian32(bits).to_ne_bytes().to_vec();
        buf.resize(8, 0);
        let mut cbuf = buf.clone();
        let c_io = unsafe {
            c::cmsOpenIOhandlerFromMem(ptr::null_mut(), cbuf.as_mut_ptr(), 8, b"r\0".as_ptr())
        };
        let mut cf = 0f32;
        let c_ok = unsafe { c::_cmsReadFloat32Number(c_io, &mut cf) };
        unsafe { c::cmsCloseIOhandler(c_io) };

        let mut io = io_handler_from_mem(None, buf, "r").unwrap();
        let mut rf = 0f32;
        let r_ok = read_float32(&mut io, &mut rf) as i32;
        assert_eq!(r_ok, c_ok, "bits={bits:#x} rng={s}");
    }
}
