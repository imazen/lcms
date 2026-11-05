//! `cmsio0.c` — ICC profile open/read/write machinery.
//!
//! Pure-Rust port: `_cmsICCPROFILE` state + `_cmsReadHeader`/`_cmsWriteHeader`,
//! the tag dictionary (names/links/sizes/offsets), cooked `cmsReadTag`/
//! `cmsWriteTag`, raw tag access, and `cmsOpenProfile*` constructors.

use crate::context::Context;
use crate::err;
use crate::io::{self, IoHandler};
use crate::tagtypes::{
    TagDescriptor, TagObject, avoid_type_check_on_tags, get_tag_descriptor, get_tag_type_handler,
};
use crate::types::{DateTimeNumber, ProfileId};

pub const MAX_TABLE_TAG: usize = 100;
pub const MAX_TYPES_IN_LCMS_PLUGIN: usize = 20;
const CMS_MAGIC: u32 = 0x61637370; // 'acsp'
const DEFAULT_MAX_VERSION: u32 = 0x04400000;
const ERROR_BAD_SIGNATURE: i32 = 5;
const ERROR_RANGE: i32 = 12;
const ERROR_UNKNOWN_EXTENSION: i32 = 4;
const ERROR_CORRUPTION_DETECTED: i32 = 13;
const ERROR_ALREADY_DEFINED: i32 = 15;

/// `_cmsICCPROFILE` — the open-profile state object.
pub struct IccProfile {
    /// `IOhandler` — the backing stream (None = built-in/virtual profile)
    pub io: Option<IoHandler>,
    /// `ContextID` — opaque context; None = global
    pub ctx: Option<Context>,
    /// `Created` — creation timestamp
    pub created: DateTimeNumber,
    /// `CMM` — cmm signature
    pub cmm: u32,
    /// `Version` — as `cmsGetProfileVersion` (e.g. 0x04300000)
    pub version: u32,
    /// `DeviceClass`
    pub device_class: u32,
    /// `ColorSpace` signature
    pub color_space: u32,
    /// `PCS` signature
    pub pcs: u32,
    /// `RenderingIntent` (u32 as in the header)
    pub rendering_intent: u32,
    pub platform: u32,
    pub flags: u32,
    pub manufacturer: u32,
    pub model: u32,
    pub attributes: u64,
    pub creator: u32,
    pub profile_id: ProfileId,

    // Tag dictionary
    pub tag_count: u32,
    pub tag_names: [u32; MAX_TABLE_TAG],
    pub tag_linked: [u32; MAX_TABLE_TAG],
    pub tag_sizes: [u32; MAX_TABLE_TAG],
    pub tag_offsets: [u32; MAX_TABLE_TAG],
    /// `TagSaveAsRaw`
    pub tag_save_as_raw: [bool; MAX_TABLE_TAG],
    /// `TagPtrs` — cooked objects (dup'd by the type handler)
    pub tag_ptrs: Vec<Option<TagObject>>,
    /// `TagTypeHandlers` — the type signature seen per tag
    pub tag_type_sigs: [u32; MAX_TABLE_TAG],
    /// Raw tag payload when written via `cmsWriteRawTag` (save-as-raw)
    pub tag_raw: Vec<Option<Vec<u8>>>,
}

impl Default for IccProfile {
    fn default() -> Self {
        Self {
            io: None,
            ctx: None,
            created: DateTimeNumber::default(),
            cmm: 0,
            version: 0x02100000,
            device_class: 0,
            color_space: 0,
            pcs: 0,
            rendering_intent: 0,
            platform: 0,
            flags: 0,
            manufacturer: 0,
            model: 0,
            attributes: 0,
            creator: 0,
            profile_id: ProfileId::default(),
            tag_count: 0,
            tag_names: [0; MAX_TABLE_TAG],
            tag_linked: [0; MAX_TABLE_TAG],
            tag_sizes: [0; MAX_TABLE_TAG],
            tag_offsets: [0; MAX_TABLE_TAG],
            tag_save_as_raw: [false; MAX_TABLE_TAG],
            tag_ptrs: vec![None; MAX_TABLE_TAG],
            tag_type_sigs: [0; MAX_TABLE_TAG],
            tag_raw: vec![None; MAX_TABLE_TAG],
        }
    }
}

/// `_validatedVersion` — operate on the big-endian bytes
fn validated_version(dword: u32) -> u32 {
    let mut p = dword.to_be_bytes();
    if p[0] > 0x09 {
        p[0] = 0x09;
    }
    let mut t1 = p[1] & 0xf0;
    let mut t2 = p[1] & 0x0f;
    if t1 > 0x90 {
        t1 = 0x90;
    }
    if t2 > 0x09 {
        t2 = 0x09;
    }
    p[1] = t1 | t2;
    p[2] = 0;
    p[3] = 0;
    u32::from_be_bytes(p)
}

/// `validDeviceClass`
fn valid_device_class(cl: u32) -> bool {
    use crate::signatures::profileclass as dc;
    cl == 0
        || matches!(
            cl,
            x if x == dc::SigInputClass
                || x == dc::SigDisplayClass
                || x == dc::SigOutputClass
                || x == dc::SigLinkClass
                || x == dc::SigAbstractClass
                || x == dc::SigColorSpaceClass
                || x == dc::SigNamedColorClass
                || x == dc::SigColorEncodingSpaceClass
                || x == dc::SigMultiplexIdentificationClass
                || x == dc::SigMultiplexLinkClass
                || x == dc::SigMultiplexVisualizationClass
        )
}

/// `SupportedICCVersion` — plugin chunk else default
fn supported_icc_version(_ctx: Option<&Context>) -> u32 {
    DEFAULT_MAX_VERSION
}

/// `IsTypeSupported`
fn is_type_supported(desc: &TagDescriptor, typ: u32) -> bool {
    let n = desc.supported_types.len().min(MAX_TYPES_IN_LCMS_PLUGIN);
    desc.supported_types[..n].contains(&typ)
}

/// `CompatibleTypes` — link-compat check
fn compatible_types(a: Option<TagDescriptor>, b: Option<TagDescriptor>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    a.elem_count == b.elem_count && a.supported_types == b.supported_types
}

/// `SearchOneTag` — position of `sig` or -1
fn search_one_tag(icc: &IccProfile, sig: u32) -> i32 {
    (0..icc.tag_count as usize)
        .find(|&i| icc.tag_names[i] == sig)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// `_cmsSearchTag` — position or -1; `follow_links` resolves TagLinked chains
pub fn search_tag(icc: &IccProfile, mut sig: u32, follow_links: bool) -> i32 {
    loop {
        let n = search_one_tag(icc, sig);
        if n < 0 {
            return -1;
        }
        if !follow_links {
            return n;
        }
        let linked = icc.tag_linked[n as usize];
        if linked == 0 {
            return n;
        }
        sig = linked;
    }
}

/// `_cmsReadHeader` — parse + validate the profile header & tag directory
pub fn read_header(icc: &mut IccProfile) -> bool {
    let io = icc.io.as_mut().unwrap();

    let mut hdr = [0u8; 128];
    if io.read(&mut hdr, 128, 1) != 1 {
        return false;
    }
    if u32::from_be_bytes([hdr[36], hdr[37], hdr[38], hdr[39]]) != CMS_MAGIC {
        err::signal_error(
            None,
            ERROR_BAD_SIGNATURE,
            "not an ICC profile, invalid signature",
        );
        return false;
    }

    let be32 = |o: usize| u32::from_be_bytes([hdr[o], hdr[o + 1], hdr[o + 2], hdr[o + 3]]);
    icc.cmm = be32(4);
    icc.device_class = be32(12);
    icc.color_space = be32(16);
    icc.pcs = be32(20);
    icc.rendering_intent = be32(64);
    icc.platform = be32(40);
    icc.flags = be32(44);
    icc.manufacturer = be32(48);
    icc.model = be32(52);
    icc.creator = be32(80);
    icc.attributes = u64::from_be_bytes(hdr[56..64].try_into().unwrap());
    icc.version = validated_version(be32(8));

    if icc.version > supported_icc_version(icc.ctx.as_ref()) {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported profile version");
        return false;
    }
    if !valid_device_class(icc.device_class) {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported device class");
        return false;
    }

    let mut header_size = be32(0); // size field
    if header_size >= io.reported_size {
        header_size = io.reported_size;
    }

    // date time (12 bytes at offset 24)
    icc.created = DateTimeNumber {
        year: u16::from_be_bytes([hdr[24], hdr[25]]),
        month: u16::from_be_bytes([hdr[26], hdr[27]]),
        day: u16::from_be_bytes([hdr[28], hdr[29]]),
        hours: u16::from_be_bytes([hdr[30], hdr[31]]),
        minutes: u16::from_be_bytes([hdr[32], hdr[33]]),
        seconds: u16::from_be_bytes([hdr[34], hdr[35]]),
    };
    icc.profile_id.id8.copy_from_slice(&hdr[84..100]);

    // Tag directory
    let mut tag_count = 0u32;
    if !io::read_u32(io, &mut tag_count) {
        return false;
    }
    if tag_count > MAX_TABLE_TAG as u32 {
        err::signal_error(None, ERROR_RANGE, "Too many tags");
        return false;
    }

    icc.tag_count = 0;
    let mut corrupted = false;
    for _ in 0..tag_count {
        let (mut sig, mut off, mut size) = (0u32, 0u32, 0u32);
        if !io::read_u32(io, &mut sig)
            || !io::read_u32(io, &mut off)
            || !io::read_u32(io, &mut size)
        {
            return false;
        }
        if size == 0 || off == 0 {
            continue;
        }
        if off.wrapping_add(size) > header_size || off.wrapping_add(size) < off {
            corrupted = true;
            continue;
        }
        let t = icc.tag_count as usize;
        icc.tag_names[t] = sig;
        icc.tag_offsets[t] = off;
        icc.tag_sizes[t] = size;

        // Search for links
        for j in 0..t {
            if icc.tag_offsets[j] == off && icc.tag_sizes[j] == size {
                let d1 = get_tag_descriptor(icc.ctx.as_ref(), icc.tag_names[j]);
                let d2 = get_tag_descriptor(icc.ctx.as_ref(), sig);
                if compatible_types(d1, d2) {
                    icc.tag_linked[t] = icc.tag_names[j];
                }
            }
        }
        icc.tag_count += 1;
    }

    // Duplicate tag names are forbidden
    for i in 0..icc.tag_count as usize {
        for j in 0..icc.tag_count as usize {
            if i != j && icc.tag_names[i] == icc.tag_names[j] {
                err::signal_error(None, ERROR_RANGE, "Duplicate tag found");
                return false;
            }
        }
    }
    if corrupted {
        err::signal_error(
            None,
            ERROR_CORRUPTION_DETECTED,
            "'size' field in header seems incorrect.",
        );
    }
    true
}

/// `_cmsWriteHeader` — serialize header + tag directory
pub fn write_header(icc: &IccProfile, used_space: u32, io_out: &mut IoHandler) -> bool {
    if icc.version > supported_icc_version(icc.ctx.as_ref()) {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported profile version");
        return false;
    }
    // Build cmsICCHeader exactly per its field layout
    let mut h = [0u8; 128];
    h[0..4].copy_from_slice(&used_space.to_be_bytes());
    h[4..8].copy_from_slice(&icc.cmm.to_be_bytes());
    h[8..12].copy_from_slice(&icc.version.to_be_bytes());
    h[12..16].copy_from_slice(&icc.device_class.to_be_bytes());
    h[16..20].copy_from_slice(&icc.color_space.to_be_bytes());
    h[20..24].copy_from_slice(&icc.pcs.to_be_bytes());
    let d = &icc.created;
    h[24..26].copy_from_slice(&d.year.to_be_bytes());
    h[26..28].copy_from_slice(&d.month.to_be_bytes());
    h[28..30].copy_from_slice(&d.day.to_be_bytes());
    h[30..32].copy_from_slice(&d.hours.to_be_bytes());
    h[32..34].copy_from_slice(&d.minutes.to_be_bytes());
    h[34..36].copy_from_slice(&d.seconds.to_be_bytes());
    h[36..40].copy_from_slice(&CMS_MAGIC.to_be_bytes());
    h[40..44].copy_from_slice(&icc.platform.to_be_bytes());
    h[44..48].copy_from_slice(&icc.flags.to_be_bytes());
    h[48..52].copy_from_slice(&icc.manufacturer.to_be_bytes());
    h[52..56].copy_from_slice(&icc.model.to_be_bytes());
    h[56..64].copy_from_slice(&icc.attributes.to_be_bytes());
    h[64..68].copy_from_slice(&icc.rendering_intent.to_be_bytes());
    // illuminant D50: X=0.9642 Y=1.0 Z=0.8249 as s15Fixed16
    h[68..72].copy_from_slice(&0x0000F6D6u32.to_be_bytes());
    h[72..76].copy_from_slice(&0x00010000u32.to_be_bytes());
    h[76..80].copy_from_slice(&0x0000D32Du32.to_be_bytes());
    h[80..84].copy_from_slice(&icc.creator.to_be_bytes());
    h[84..100].copy_from_slice(&icc.profile_id.id8);
    // remaining bytes = reserved zeros

    if !io_out.write(128, &h) {
        return false;
    }
    true
}

/// `_cmsSearchTag` for `cmsReadTag`/`cmsWriteTag` — internal position
pub fn cms_is_tag(icc: &IccProfile, sig: u32) -> bool {
    search_tag(icc, sig, false) >= 0
}

/// `cmsReadTag` — cooked read through the type handler
pub fn cms_read_tag(icc: &mut IccProfile, sig: u32) -> Option<&TagObject> {
    let ctx = icc.ctx.as_ref();
    let avoid_check = avoid_type_check_on_tags(ctx);

    let n = search_tag(icc, sig, true);
    if n < 0 {
        return None;
    }
    let n = n as usize;

    // Already in memory
    if icc.tag_ptrs[n].is_some() {
        if icc.tag_type_sigs[n] == 0 {
            return None;
        }
        let base_type = icc.tag_type_sigs[n];
        if !avoid_check {
            let Some(desc) = get_tag_descriptor(ctx, sig) else {
                return None;
            };
            if !is_type_supported(&desc, base_type) {
                return None;
            }
        }
        if icc.tag_save_as_raw[n] {
            return None; // cooked read of raw tag unsupported
        }
        return icc.tag_ptrs[n].as_ref();
    }

    let offset = icc.tag_offsets[n];
    let mut tag_size = icc.tag_sizes[n];
    if tag_size < 8 {
        return None;
    }
    let Some(io) = icc.io.as_mut() else {
        err::signal_error(
            None,
            ERROR_CORRUPTION_DETECTED,
            "Corrupted built-in profile.",
        );
        return None;
    };
    if !io.seek(offset) {
        return None;
    }
    let desc = if !avoid_check {
        match get_tag_descriptor(ctx, sig) {
            Some(d) => Some(d),
            None => {
                err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown tag type found.");
                return None;
            }
        }
    } else {
        None
    };
    let base_type = io::read_type_base(io);
    if base_type == 0 {
        return None;
    }
    if !avoid_check && !is_type_supported(&desc.unwrap(), base_type) {
        return None;
    }
    tag_size -= 8;

    let Some(handler) = get_tag_type_handler(ctx, base_type) else {
        return None;
    };
    icc.tag_type_sigs[n] = handler.signature;
    let mut local = handler;
    local.icc_version = icc.version;
    let mut elem_count = 0u32;
    let obj = (local.read)(&local, icc.io.as_mut().unwrap(), &mut elem_count, tag_size);
    match obj {
        Some(o) => {
            if let Some(d) = desc {
                if !avoid_check && elem_count < d.elem_count {
                    err::signal_error(
                        None,
                        ERROR_CORRUPTION_DETECTED,
                        "inconsistent number of items",
                    );
                    icc.tag_ptrs[n] = None;
                    return None;
                }
            }
            icc.tag_ptrs[n] = Some(o);
            icc.tag_ptrs[n].as_ref()
        }
        None => {
            err::signal_error(None, ERROR_CORRUPTION_DETECTED, "Corrupted tag");
            icc.tag_ptrs[n] = None;
            None
        }
    }
}

/// `cmsWriteTag` — cooked write (stores the object, serializes on save)
pub fn cms_write_tag(icc: &mut IccProfile, sig: u32, data: Option<TagObject>) -> bool {
    let ctx = icc.ctx.as_ref();
    // Delete path
    let Some(obj) = data else {
        let i = search_tag(icc, sig, false);
        if i < 0 {
            return false;
        }
        let i = i as usize;
        // delete the tag: shift all entries down
        for k in i..icc.tag_count as usize - 1 {
            icc.tag_names[k] = icc.tag_names[k + 1];
            icc.tag_linked[k] = icc.tag_linked[k + 1];
            icc.tag_sizes[k] = icc.tag_sizes[k + 1];
            icc.tag_offsets[k] = icc.tag_offsets[k + 1];
            icc.tag_save_as_raw[k] = icc.tag_save_as_raw[k + 1];
            icc.tag_ptrs[k] = icc.tag_ptrs[k + 1].take();
            icc.tag_type_sigs[k] = icc.tag_type_sigs[k + 1];
            icc.tag_raw[k] = icc.tag_raw[k + 1].take();
        }
        icc.tag_count -= 1;
        let last = icc.tag_count as usize;
        icc.tag_names[last] = 0;
        icc.tag_ptrs[last] = None;
        icc.tag_raw[last] = None;
        return true;
    };

    // New tag slot
    let i = match search_tag(icc, sig, false) {
        -1 => {
            if icc.tag_count as usize >= MAX_TABLE_TAG {
                err::signal_error(None, ERROR_RANGE, "Too many tags");
                return false;
            }
            let t = icc.tag_count as usize;
            icc.tag_count += 1;
            t
        }
        i => i as usize,
    };

    if icc.tag_save_as_raw[i] {
        err::signal_error(None, ERROR_ALREADY_DEFINED, "Tag was already saved as RAW");
        return false;
    }
    icc.tag_linked[i] = 0;

    let Some(desc) = get_tag_descriptor(ctx, sig) else {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported tag");
        return false;
    };

    // Decide the type from descriptor + object
    let ver = {
        let n = icc.version >> 16;
        // BaseToBase(n, 16, 10)/100 — BCD digit decode
        let s = format!("{n:x}");
        s.parse::<f64>().unwrap_or(0.0) / 100.0
    };
    let typ = match desc.decide_type {
        Some(d) => d(ver, &obj),
        None => desc.supported_types[0],
    };
    if !is_type_supported(&desc, typ) {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported type for tag");
        return false;
    }
    let Some(handler) = get_tag_type_handler(ctx, typ) else {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unsupported type for tag");
        return false;
    };

    icc.tag_type_sigs[i] = handler.signature;
    icc.tag_names[i] = sig;
    icc.tag_sizes[i] = 0;
    icc.tag_offsets[i] = 0;
    icc.tag_save_as_raw[i] = false;
    icc.tag_ptrs[i] = Some(obj); // DupPtr absorbed by Clone semantics
    icc.tag_raw[i] = None;
    true
}

/// `cmsOpenProfileFromMemTHR` — open a profile from a memory buffer
pub fn open_profile_from_mem(ctx: Option<Context>, data: &[u8]) -> Option<Box<IccProfile>> {
    let io = io::io_handler_from_mem(None, data.to_vec(), "r")?;
    let mut icc = Box::new(IccProfile {
        io: Some(io),
        ctx,
        ..Default::default()
    });
    if !read_header(&mut icc) {
        return None;
    }
    Some(icc)
}

/// `cmsCreateProfilePlaceholder` — an empty write-side profile
pub fn create_profile_placeholder(ctx: Option<Context>) -> Box<IccProfile> {
    let mut icc = Box::new(IccProfile::default());
    icc.ctx = ctx;
    icc.device_class = crate::signatures::profileclass::SigAbstractClass;
    icc.color_space = 0;
    icc.pcs = crate::signatures::colorspace::SigXYZData;
    icc.rendering_intent = 0;
    icc.platform = 0;
    icc.flags = 0;
    icc.manufacturer = 0;
    icc.model = 0;
    icc.attributes = 0;
    icc.creator = 0;
    icc.version = 0x02100000;
    // created = current time — deterministic for port: leave epoch
    icc
}

/// `cmsIsTag`
pub fn cms_is_tag_pub(icc: &IccProfile, sig: u32) -> bool {
    search_tag(icc, sig, false) >= 0
}

/// Serialize ONE tag's cooked data (type base + body)
fn serialize_tag(ctx: Option<&Context>, icc: &IccProfile, n: usize) -> Option<Vec<u8>> {
    if let Some(raw) = &icc.tag_raw[n] {
        return Some(raw.clone());
    }
    let Some(obj) = icc.tag_ptrs[n].as_ref() else {
        eprintln!("st: no obj");
        return None;
    };
    let sig = icc.tag_type_sigs[n];
    let Some(handler) = get_tag_type_handler(ctx, sig) else {
        eprintln!("st: no handler {sig:#x}");
        return None;
    };
    let mut local = handler;
    local.icc_version = icc.version;

    // Pass 1: NULL handler for the byte count
    let mut null_io = io::io_handler_from_null(ctx);
    if !io::write_type_base(&mut null_io, sig) || !(local.write)(&local, &mut null_io, obj, 1) {
        return None;
    }
    let size = null_io.tell() as usize;

    // Pass 2: real buffer
    let mut mem = io::io_handler_from_mem(None, vec![0u8; size], "w")?;
    if !io::write_type_base(&mut mem, sig) || !(local.write)(&local, &mut mem, obj, 1) {
        return None;
    }
    match mem.stream {
        io::IoStream::Mem(m) => Some(m.block[..size].to_vec()),
        _ => None,
    }
}

/// `cmsSaveProfileToMem` — write the whole profile to memory
pub fn save_profile_to_mem(icc: &mut IccProfile) -> Option<Vec<u8>> {
    let ctx = icc.ctx.as_ref().map(|c| {
        // SAFETY-FREE: ctx lives inside icc — copy via clone for the handler call
        c
    });
    // Serialize each cooked tag
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(icc.tag_count as usize);
    for n in 0..icc.tag_count as usize {
        if icc.tag_names[n] == 0 {
            bodies.push(Vec::new());
            continue;
        }
        let body = if icc.tag_linked[n] != 0 {
            let j = search_tag(icc, icc.tag_linked[n], false);
            if j < 0 {
                return None;
            }
            serialize_tag(ctx, icc, j as usize)?
        } else {
            serialize_tag(ctx, icc, n)?
        };
        bodies.push(body);
    }

    // Layout: header(128) + count(4) + dir(12*n) + bodies (4-aligned)
    let n_tags = icc.tag_count as usize;
    let mut offsets = vec![0u32; n_tags];
    let mut pos = 128 + 4 + 12 * n_tags;
    for (i, body) in bodies.iter().enumerate() {
        offsets[i] = pos as u32;
        icc.tag_offsets[i] = pos as u32;
        icc.tag_sizes[i] = body.len() as u32;
        pos += (body.len() + 3) & !3;
    }

    // Write everything through one mem handler (exercises the real IO path)
    let mut out_io = io::io_handler_from_mem(None, vec![0u8; pos], "w")?;
    if !write_header(icc, pos as u32, &mut out_io) {
        return None;
    }
    if !io::write_u32(&mut out_io, icc.tag_count) {
        return None;
    }
    for i in 0..n_tags {
        if !io::write_u32(&mut out_io, icc.tag_names[i])
            || !io::write_u32(&mut out_io, offsets[i])
            || !io::write_u32(&mut out_io, icc.tag_sizes[i])
        {
            return None;
        }
    }
    for (i, body) in bodies.iter().enumerate() {
        if !out_io.seek(offsets[i]) || !out_io.write(body.len() as u32, body) {
            return None;
        }
    }
    match out_io.stream {
        io::IoStream::Mem(m) => Some(m.block[..pos].to_vec()),
        _ => None,
    }
}
