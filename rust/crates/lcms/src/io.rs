//! Port of the IO layer: `cmsio0.c`'s `cmsIOHANDLER` implementations plus
//! `cmsplugin.c`'s endian/read-write primitives.
//!
//! C sources: `reference/lcms2/src/cmsio0.c` (handlers), `cmsplugin.c`
//! (`_cmsAdjustEndianess*`, `_cms{Read,Write}*Number`, type base, alignment).
//! Gate: `tests/c_parity_io.rs`
//!
//! `io->Read/Seek/Close/Tell/Write` fn pointers → methods dispatching on the
//! `IoStream` enum (same semantics; plugin-supplied handlers get a `Custom`
//! variant when the plugin API lands). `iohandler->stream` ownership is
//! identical to C: File owns the `std::fs::File`, Mem owns the block.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::context::Context;
use crate::err;
use crate::fixed::{double_to_s15_fixed16, s15_fixed16_to_double};
use crate::types::{CieXyz, ERROR_FILE, ERROR_READ};

/// `cmsMAX_PATH`
const MAX_PATH: usize = 256;
/// `cmsMagicNumber` — 'acsp'
pub const MAGIC_NUMBER: u32 = 0x61637370;

/// `_cmsAdjustEndianess16`
#[inline]
pub fn adjust_endian16(w: u16) -> u16 {
    w.to_be()
}

/// `_cmsAdjustEndianess32`
#[inline]
pub fn adjust_endian32(v: u32) -> u32 {
    v.to_be()
}

/// `_cmsAdjustEndianess64`
#[inline]
pub fn adjust_endian64(q: u64) -> u64 {
    q.to_be()
}

/// `_cmsALIGNLONG`
#[inline]
pub fn align_long(x: u32) -> u32 {
    x.wrapping_add(3) & !3u32
}

/// `FILEMEM` — memory stream state.
#[derive(Clone)]
pub struct FileMem {
    pub block: Vec<u8>,
    pub size: u32,
    pub pointer: u32,
    pub free_block_on_close: bool,
}

/// `iohandler->stream` — the stream implementations.
pub enum IoStream {
    /// `cmsOpenIOhandlerFromFile`/`FromStream` — wraps std File; BufReader
    /// so the per-element u16/u32 tag reads aren't one syscall each.
    File(std::io::BufReader<std::fs::File>),
    /// `cmsOpenIOhandlerFromMem`.
    Mem(FileMem),
    /// `cmsOpenIOhandlerFromNULL`.
    Null { pointer: u32 },
}

impl Clone for IoStream {
    fn clone(&self) -> Self {
        match self {
            IoStream::File(f) => IoStream::File(std::io::BufReader::new(
                f.get_ref().try_clone().expect("clone file stream"),
            )),
            IoStream::Mem(m) => IoStream::Mem(m.clone()),
            IoStream::Null { pointer } => IoStream::Null { pointer: *pointer },
        }
    }
}

/// `struct _cms_io_handler`
#[derive(Clone)]
pub struct IoHandler {
    pub stream: IoStream,
    pub used_space: u32,
    pub reported_size: u32,
    pub physical_file: [u8; MAX_PATH],
}

impl IoHandler {
    /// `io->Read(io, Buffer, size, count)` — returns items read.
    pub fn read(&mut self, mut buf: &mut [u8], size: u32, count: u32) -> u32 {
        match &mut self.stream {
            IoStream::File(f) => {
                let mut n_readed = 0u32;
                for _ in 0..count {
                    // FileRead: fread(Buffer, size, 1, file) per item
                    let want = size as usize;
                    if f.read_exact(&mut buf[..want]).is_err() {
                        err::signal_error(
                            None,
                            ERROR_FILE,
                            &format!("Read error. Got 0 bytes, block should be of {size} bytes"),
                        );
                        return n_readed;
                    }
                    n_readed += 1;
                    buf = &mut buf[want..];
                }
                n_readed
            }
            IoStream::Mem(m) => {
                if size == 0 || count == 0 {
                    return 0;
                }
                let len = size.wrapping_mul(count);
                if len / count != size || len > m.size || m.pointer > m.size - len {
                    err::signal_error(None, ERROR_READ, "Read from memory error");
                    return 0;
                }
                let p = m.pointer as usize;
                buf[..len as usize].copy_from_slice(&m.block[p..p + len as usize]);
                m.pointer += len;
                count
            }
            IoStream::Null { .. } => 0,
        }
    }

    /// `io->Seek(io, offset)`
    pub fn seek(&mut self, offset: u32) -> bool {
        match &mut self.stream {
            IoStream::File(f) => match f.seek(SeekFrom::Start(offset as u64)) {
                Ok(_) => true,
                Err(_) => {
                    err::signal_error(None, ERROR_FILE, "Seek error; probably corrupted file");
                    false
                }
            },
            IoStream::Mem(m) => {
                if offset > m.size {
                    return false;
                }
                m.pointer = offset;
                true
            }
            IoStream::Null { pointer } => {
                *pointer = offset;
                true
            }
        }
    }

    /// `io->Tell(io)`
    pub fn tell(&mut self) -> u32 {
        match &mut self.stream {
            IoStream::File(f) => f.stream_position().map(|p| p as u32).unwrap_or_else(|_| {
                err::signal_error(None, ERROR_FILE, "Tell error; probably corrupted file");
                0
            }),
            IoStream::Mem(m) => m.pointer,
            IoStream::Null { pointer } => *pointer,
        }
    }

    /// `io->Write(io, size, Buffer)`
    pub fn write(&mut self, size: u32, buf: &[u8]) -> bool {
        match &mut self.stream {
            IoStream::File(f) => {
                if size == 0 {
                    return true;
                }
                self.used_space = self.used_space.wrapping_add(size);
                f.get_mut().write_all(&buf[..size as usize]).is_ok()
            }
            IoStream::Mem(m) => {
                if size == 0 {
                    return true;
                }
                // Truncate the output in case the space is not enough
                let mut sz = size;
                if sz > m.size - m.pointer {
                    sz = m.size - m.pointer;
                }
                m.block[m.pointer as usize..(m.pointer + sz) as usize]
                    .copy_from_slice(&buf[..sz as usize]);
                m.pointer += sz;
                if m.pointer > self.used_space {
                    self.used_space = m.pointer;
                }
                true
            }
            IoStream::Null { pointer } => {
                if size > 0xFFFF_FFFFu32 - *pointer {
                    return false;
                }
                *pointer += size;
                if *pointer > self.used_space {
                    self.used_space = *pointer;
                }
                true
            }
        }
    }
}

/// `cmsOpenIOhandlerFromNULL`
pub fn io_handler_from_null(_ctx: Option<&Context>) -> IoHandler {
    IoHandler {
        stream: IoStream::Null { pointer: 0 },
        used_space: 0,
        reported_size: 0,
        physical_file: [0; MAX_PATH],
    }
}

/// `cmsOpenIOhandlerFromMem` — the block is owned (caller gets it back via
/// `into_mem_block`, matching `FreeBlockOnClose` semantics).
pub fn io_handler_from_mem(
    _ctx: Option<&Context>,
    buffer: Vec<u8>,
    access: &str,
) -> Option<IoHandler> {
    let size = buffer.len() as u32;
    let mut fm = FileMem {
        block: buffer,
        size: 0,
        pointer: 0,
        free_block_on_close: false,
    };

    if access.starts_with('r') || access.starts_with('R') {
        fm.size = size;
    } else if access.starts_with('w') || access.starts_with('W') {
        // write mode — buffer is the storage medium
        fm.size = size;
    } else {
        return None;
    }

    Some(IoHandler {
        reported_size: size,
        stream: IoStream::Mem(fm),
        used_space: 0,
        physical_file: [0; MAX_PATH],
    })
}

/// `cmsOpenIOhandlerFromFile` — `access` like C's "r"/"w" (+ optional 'e').
pub fn io_handler_from_file(_ctx: Option<&Context>, path: &str, access: &str) -> Option<IoHandler> {
    let mut mode = [0u8; 4];
    for &b in access.as_bytes() {
        match b {
            b'r' | b'w' => {
                if mode[0] == 0 {
                    mode[0] = b;
                    mode[1] = b'b';
                } else {
                    err::signal_error(None, ERROR_FILE, "Access mode already specified");
                    return None;
                }
            }
            b'e' => mode[2] = b'e',
            _ => {
                err::signal_error(None, ERROR_FILE, "Wrong access mode");
                return None;
            }
        }
    }

    let (f, reported) = match mode[0] {
        b'r' => {
            let f = std::io::BufReader::new(match std::fs::File::open(path) {
                Ok(f) => f,
                Err(_) => {
                    err::signal_error(None, ERROR_FILE, &format!("File '{path}' not found"));
                    return None;
                }
            });
            let len = f.get_ref().metadata().ok()?.len();
            if len > u32::MAX as u64 {
                err::signal_error(None, ERROR_FILE, "File is too large");
                return None;
            }
            (f, len as u32)
        }
        b'w' => (
            std::io::BufReader::new(std::fs::File::create(path).ok()?),
            0,
        ),
        _ => return None,
    };

    let mut physical_file = [0u8; MAX_PATH];
    let pb = path.as_bytes();
    let n = pb.len().min(MAX_PATH - 1);
    physical_file[..n].copy_from_slice(&pb[..n]);

    Some(IoHandler {
        stream: IoStream::File(f),
        used_space: 0,
        reported_size: reported,
        physical_file,
    })
}

// ---------------------------------------------------------------------------
// Read primitives (cmsplugin.c)
// ---------------------------------------------------------------------------

/// `_cmsReadUInt8Number`
pub fn read_u8(io: &mut IoHandler, n: &mut u8) -> bool {
    let mut tmp = [0u8; 1];
    if io.read(&mut tmp, 1, 1) != 1 {
        return false;
    }
    *n = tmp[0];
    true
}

/// `_cmsReadUInt16Number`
pub fn read_u16(io: &mut IoHandler, n: &mut u16) -> bool {
    let mut tmp = [0u8; 2];
    if io.read(&mut tmp, 2, 1) != 1 {
        return false;
    }
    *n = adjust_endian16(u16::from_ne_bytes(tmp));
    true
}

/// `_cmsReadUInt16Array` — `None` array means skip (C passes NULL).
pub fn read_u16_array(io: &mut IoHandler, n: u32, mut array: Option<&mut [u16]>) -> bool {
    for i in 0..n as usize {
        match &mut array {
            Some(a) => {
                if !read_u16(io, &mut a[i]) {
                    return false;
                }
            }
            None => {
                let mut tmp = 0u16;
                if !read_u16(io, &mut tmp) {
                    return false;
                }
            }
        }
    }
    true
}

/// `_cmsReadUInt32Number`
pub fn read_u32(io: &mut IoHandler, n: &mut u32) -> bool {
    let mut tmp = [0u8; 4];
    if io.read(&mut tmp, 4, 1) != 1 {
        return false;
    }
    *n = adjust_endian32(u32::from_ne_bytes(tmp));
    true
}

/// `_cmsReadFloat32Number` — includes the sanity checks.
pub fn read_float32(io: &mut IoHandler, n: &mut f32) -> bool {
    let mut tmp = [0u8; 4];
    if io.read(&mut tmp, 4, 1) != 1 {
        return false;
    }
    *n = f32::from_bits(adjust_endian32(u32::from_ne_bytes(tmp)));

    // Safeguard which covers against absurd values
    if *n > 1e20 || *n < -1e20 {
        return false;
    }
    // fpclassify == FP_ZERO || FP_NORMAL
    n.is_normal() || *n == 0.0
}

/// `_cmsReadUInt64Number`
pub fn read_u64(io: &mut IoHandler, n: &mut u64) -> bool {
    let mut tmp = [0u8; 8];
    if io.read(&mut tmp, 8, 1) != 1 {
        return false;
    }
    *n = adjust_endian64(u64::from_ne_bytes(tmp));
    true
}

/// `_cmsRead15Fixed16Number`
pub fn read_15_fixed16(io: &mut IoHandler, n: &mut f64) -> bool {
    let mut tmp = [0u8; 4];
    if io.read(&mut tmp, 4, 1) != 1 {
        return false;
    }
    *n = s15_fixed16_to_double(adjust_endian32(u32::from_ne_bytes(tmp)) as i32);
    true
}

/// `_cmsReadXYZNumber`
pub fn read_xyz(io: &mut IoHandler, xyz: &mut CieXyz) -> bool {
    let mut tmp = [0u8; 12];
    if io.read(&mut tmp, 12, 1) != 1 {
        return false;
    }
    xyz.X = s15_fixed16_to_double(
        adjust_endian32(u32::from_ne_bytes(tmp[0..4].try_into().unwrap())) as i32,
    );
    xyz.Y = s15_fixed16_to_double(
        adjust_endian32(u32::from_ne_bytes(tmp[4..8].try_into().unwrap())) as i32,
    );
    xyz.Z = s15_fixed16_to_double(adjust_endian32(u32::from_ne_bytes(
        tmp[8..12].try_into().unwrap(),
    )) as i32);
    true
}

/// `_cmsWriteUInt8Number`
pub fn write_u8(io: &mut IoHandler, n: u8) -> bool {
    io.write(1, &[n])
}

/// `_cmsWriteUInt16Number`
pub fn write_u16(io: &mut IoHandler, n: u16) -> bool {
    io.write(2, &adjust_endian16(n).to_ne_bytes())
}

/// `_cmsWriteUInt16Array`
pub fn write_u16_array(io: &mut IoHandler, array: &[u16]) -> bool {
    for &a in array {
        if !write_u16(io, a) {
            return false;
        }
    }
    true
}

/// `_cmsWriteUInt32Number`
pub fn write_u32(io: &mut IoHandler, n: u32) -> bool {
    io.write(4, &adjust_endian32(n).to_ne_bytes())
}

/// `_cmsWriteFloat32Number`
pub fn write_float32(io: &mut IoHandler, n: f32) -> bool {
    io.write(4, &adjust_endian32(n.to_bits()).to_ne_bytes())
}

/// `_cmsWriteUInt64Number`
pub fn write_u64(io: &mut IoHandler, n: u64) -> bool {
    io.write(8, &adjust_endian64(n).to_ne_bytes())
}

/// `_cmsWrite15Fixed16Number`
pub fn write_15_fixed16(io: &mut IoHandler, n: f64) -> bool {
    io.write(
        4,
        &adjust_endian32(double_to_s15_fixed16(n) as u32).to_ne_bytes(),
    )
}

/// `_cmsWriteXYZNumber`
pub fn write_xyz(io: &mut IoHandler, xyz: &CieXyz) -> bool {
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&adjust_endian32(double_to_s15_fixed16(xyz.X) as u32).to_ne_bytes());
    buf[4..8].copy_from_slice(&adjust_endian32(double_to_s15_fixed16(xyz.Y) as u32).to_ne_bytes());
    buf[8..12].copy_from_slice(&adjust_endian32(double_to_s15_fixed16(xyz.Z) as u32).to_ne_bytes());
    io.write(12, &buf)
}

/// `_cmsTagBase` — read type signature + 4 reserved bytes.
pub fn read_type_base(io: &mut IoHandler) -> u32 {
    let mut buf = [0u8; 8];
    if io.read(&mut buf, 8, 1) != 1 {
        return 0;
    }
    adjust_endian32(u32::from_ne_bytes(buf[0..4].try_into().unwrap()))
}

/// `_cmsWriteTypeBase`
pub fn write_type_base(io: &mut IoHandler, sig: u32) -> bool {
    let mut buf = [0u8; 8];
    buf[0..4].copy_from_slice(&adjust_endian32(sig).to_ne_bytes());
    io.write(8, &buf)
}

/// `_cmsReadAlignment`
pub fn read_alignment(io: &mut IoHandler) -> bool {
    let at = io.tell();
    let next = align_long(at);
    let to_next = next - at;
    if to_next == 0 {
        return true;
    }
    if to_next > 4 {
        return false;
    }
    let mut buf = [0u8; 4];
    io.read(&mut buf[..to_next as usize], to_next, 1) == 1
}

/// `_cmsWriteAlignment`
pub fn write_alignment(io: &mut IoHandler) -> bool {
    let at = io.tell();
    let next = align_long(at);
    let to_next = next - at;
    if to_next == 0 {
        return true;
    }
    if to_next > 4 {
        return false;
    }
    io.write(to_next, &[0u8; 4])
}
