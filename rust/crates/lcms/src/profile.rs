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
use crate::types::{
    ERROR_ALREADY_DEFINED, ERROR_BAD_SIGNATURE, ERROR_CORRUPTION_DETECTED, ERROR_INTERNAL,
    ERROR_RANGE, ERROR_UNKNOWN_EXTENSION, ERROR_WRITE,
};

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

    // Tag directory — count of non-placeholder entries, then sig/off/size
    let count = (0..icc.tag_count as usize)
        .filter(|&i| icc.tag_names[i] != 0)
        .count() as u32;
    if !io::write_u32(io_out, count) {
        return false;
    }
    for i in 0..icc.tag_count as usize {
        if icc.tag_names[i] == 0 {
            continue;
        }
        if !io::write_u32(io_out, icc.tag_names[i])
            || !io::write_u32(io_out, icc.tag_offsets[i])
            || !io::write_u32(io_out, icc.tag_sizes[i])
        {
            return false;
        }
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
    // Delete path — C marks the slot (TagNames[i]=0), it does NOT compact
    let Some(obj) = data else {
        let i = search_tag(icc, sig, false);
        if i < 0 {
            return false;
        }
        let i = i as usize;
        delete_tag_by_pos(icc, i);
        icc.tag_names[i] = 0;
        return true;
    };

    let Some(i) = new_tag(icc, sig) else {
        return false;
    };

    if icc.tag_save_as_raw[i] {
        err::signal_error(None, ERROR_ALREADY_DEFINED, "Tag was already saved as RAW");
        return false;
    }
    icc.tag_linked[i] = 0;

    let ctx = icc.ctx.as_ref();
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

/// `_cmsDeleteTagByPos` — free slot contents (no array compaction)
fn delete_tag_by_pos(icc: &mut IccProfile, i: usize) {
    icc.tag_ptrs[i] = None;
    icc.tag_raw[i] = None;
    icc.tag_save_as_raw[i] = false;
    icc.tag_type_sigs[i] = 0;
}

/// `_cmsNewTag` — reuse an existing slot (cleared) or append
fn new_tag(icc: &mut IccProfile, sig: u32) -> Option<usize> {
    let i = search_tag(icc, sig, false);
    if i >= 0 {
        let i = i as usize;
        delete_tag_by_pos(icc, i);
        return Some(i);
    }
    if icc.tag_count as usize >= MAX_TABLE_TAG {
        err::signal_error(None, ERROR_RANGE, "Too many tags");
        return None;
    }
    let t = icc.tag_count as usize;
    icc.tag_count += 1;
    Some(t)
}

/// `cmsReadRawTag` — bytes read/needed (0 = error). `data=None` → sizing.
pub fn cms_read_raw_tag(icc: &mut IccProfile, sig: u32, data: Option<&mut [u8]>) -> u32 {
    let buf_size = data.as_ref().map(|b| b.len() as u32).unwrap_or(0);
    if data.is_some() && buf_size == 0 {
        return 0;
    }
    let i = search_tag(icc, sig, true);
    if i < 0 {
        return 0;
    }
    let i = i as usize;

    // Never cooked — straight copy from disk
    if icc.tag_ptrs[i].is_none() && icc.tag_raw[i].is_none() {
        let offset = icc.tag_offsets[i];
        let mut tag_size = icc.tag_sizes[i];
        match data {
            Some(buf) => {
                if buf_size < tag_size {
                    tag_size = buf_size;
                }
                let Some(io) = icc.io.as_mut() else {
                    return 0;
                };
                if !io.seek(offset)
                    || io.read(&mut buf[..tag_size as usize], 1, tag_size) != tag_size
                {
                    return 0;
                }
                tag_size
            }
            None => icc.tag_sizes[i],
        }
    } else if icc.tag_save_as_raw[i] {
        // Written raw — hand back the stored block
        match data {
            Some(buf) => {
                let mut tag_size = icc.tag_sizes[i];
                if buf_size < tag_size {
                    tag_size = buf_size;
                }
                buf[..tag_size as usize]
                    .copy_from_slice(&icc.tag_raw[i].as_ref().unwrap()[..tag_size as usize]);
                tag_size
            }
            None => icc.tag_sizes[i],
        }
    } else {
        // Cooked — serialize to raw
        if cms_read_tag(icc, sig).is_none() {
            return 0;
        }
        let ctx = icc.ctx.as_ref();
        let Some(desc) = get_tag_descriptor(ctx, sig) else {
            return 0;
        };
        let type_sig = icc.tag_type_sigs[i];
        if type_sig == 0 {
            return 0;
        }
        let Some(handler) = get_tag_type_handler(ctx, type_sig) else {
            return 0;
        };
        let mut local = handler;
        local.icc_version = icc.version;
        let obj = icc.tag_ptrs[i].clone().unwrap();

        match data {
            None => {
                let mut mem = io::io_handler_from_null(ctx);
                if !io::write_type_base(&mut mem, type_sig)
                    || !(local.write)(&local, &mut mem, &obj, desc.elem_count)
                {
                    return 0;
                }
                mem.tell()
            }
            Some(buf) => {
                // Sized to the caller's buffer — models C's MemoryWrite truncation
                let mut mem = io::io_handler_from_mem(None, vec![0u8; buf_size as usize], "w")
                    .unwrap_or_else(|| io::io_handler_from_null(ctx));
                if !io::write_type_base(&mut mem, type_sig)
                    || !(local.write)(&local, &mut mem, &obj, desc.elem_count)
                {
                    return 0;
                }
                let written = (mem.tell() as usize).min(buf_size as usize);
                if let io::IoStream::Mem(m) = &mem.stream {
                    buf[..written].copy_from_slice(&m.block[..written]);
                }
                mem.tell()
            }
        }
    }
}

/// `cmsWriteRawTag` — store a raw block verbatim
pub fn cms_write_raw_tag(icc: &mut IccProfile, sig: u32, data: &[u8]) -> bool {
    let Some(i) = new_tag(icc, sig) else {
        return false;
    };
    icc.tag_save_as_raw[i] = true;
    icc.tag_names[i] = sig;
    icc.tag_linked[i] = 0;
    icc.tag_type_sigs[i] = 0;
    icc.tag_ptrs[i] = None;
    icc.tag_raw[i] = Some(data.to_vec());
    icc.tag_sizes[i] = data.len() as u32;
    true
}

/// `cmsLinkTag` — point `sig` at `dest`'s block
pub fn cms_link_tag(icc: &mut IccProfile, sig: u32, dest: u32) -> bool {
    let Some(i) = new_tag(icc, sig) else {
        return false;
    };
    icc.tag_save_as_raw[i] = false;
    icc.tag_names[i] = sig;
    icc.tag_linked[i] = dest;
    icc.tag_ptrs[i] = None;
    icc.tag_raw[i] = None;
    icc.tag_sizes[i] = 0;
    icc.tag_offsets[i] = 0;
    true
}

/// `cmsTagLinkedTo`
pub fn cms_tag_linked_to(icc: &IccProfile, sig: u32) -> u32 {
    let i = search_tag(icc, sig, false);
    if i < 0 {
        return 0;
    }
    icc.tag_linked[i as usize]
}

/// `save_tags` — serialize all tag bodies into `io` (two phases share this).
/// `file_orig` = the profile's original read io for blind-copying
/// unmodified on-disk tags byte-for-byte (C's `FileOrig->IOhandler`).
fn save_tags(
    icc: &mut IccProfile,
    io: &mut IoHandler,
    file_orig: Option<&mut IoHandler>,
    orig_offsets: &[u32],
    orig_sizes: &[u32],
) -> bool {
    let ctx = icc.ctx.as_ref();
    let ver = {
        let n = icc.version >> 16;
        format!("{n:x}").parse::<f64>().unwrap_or(0.0) / 100.0
    };
    let mut file_orig = file_orig;
    let n_tags = icc.tag_count as usize;
    for i in 0..n_tags {
        if icc.tag_names[i] == 0 || icc.tag_linked[i] != 0 {
            continue;
        }
        let begin = io.used_space;
        icc.tag_offsets[i] = begin;

        if icc.tag_ptrs[i].is_none() && icc.tag_raw[i].is_none() {
            // Blind copy of an unmodified on-disk tag. C checks
            // `FileOrig != NULL && Icc->TagOffsets[i] != 0` where TagOffsets[i]
            // was just assigned Begin — the check reduces to FileOrig!=NULL.
            if let Some(src) = file_orig.as_deref_mut() {
                if orig_offsets[i] != 0 {
                    let mut mem = vec![0u8; orig_sizes[i] as usize];
                    if !src.seek(orig_offsets[i])
                        || src.read(&mut mem, orig_sizes[i], 1) != 1
                        || !io.write(orig_sizes[i], &mem)
                    {
                        return false;
                    }
                    icc.tag_sizes[i] = io.used_space.wrapping_sub(begin);
                    if !io::write_alignment(io) {
                        return false;
                    }
                }
            }
            continue;
        }

        if icc.tag_save_as_raw[i] {
            let raw = icc.tag_raw[i].as_ref().unwrap().clone();
            if !io.write(icc.tag_sizes[i], &raw) {
                return false;
            }
        } else {
            let Some(desc) = get_tag_descriptor(ctx, icc.tag_names[i]) else {
                continue;
            };
            let Some(obj) = icc.tag_ptrs[i].clone() else {
                continue;
            };
            let typ = match desc.decide_type {
                Some(d) => d(ver, &obj),
                None => desc.supported_types[0],
            };
            let Some(handler) = get_tag_type_handler(ctx, typ) else {
                err::signal_error(None, ERROR_INTERNAL, "(Internal) no handler for tag");
                continue;
            };
            if !io::write_type_base(io, handler.signature) {
                return false;
            }
            let mut local = handler;
            local.icc_version = icc.version;
            if !(local.write)(&local, io, &obj, desc.elem_count) {
                err::signal_error(None, ERROR_WRITE, "Couldn't write type");
                return false;
            }
        }
        icc.tag_sizes[i] = io.used_space.wrapping_sub(begin);
        if !io::write_alignment(io) {
            return false;
        }
    }
    true
}

/// `SetLinks` — propagate offsets/sizes to linked tags
fn set_links(icc: &mut IccProfile) -> bool {
    for i in 0..icc.tag_count as usize {
        let lnk = icc.tag_linked[i];
        if lnk != 0 {
            let j = search_tag(icc, lnk, false);
            if j >= 0 {
                let j = j as usize;
                icc.tag_offsets[i] = icc.tag_offsets[j];
                icc.tag_sizes[i] = icc.tag_sizes[j];
            }
        }
    }
    true
}

/// `cmsSaveProfileToIOhandler` — two-pass save; `out = None` computes sizes
/// only (C passes NULL). Returns bytes used, 0 on error.
pub fn save_profile_to_iohandler(icc: &mut IccProfile, out: Option<&mut IoHandler>) -> u32 {
    // Keep = pre-swap profile; its io is the blind-copy source
    let mut orig_io = icc.io.take();

    // Keep's directory = disk offsets/sizes for the blind copy (C memmove)
    let orig_offsets: Vec<u32> = icc.tag_offsets[..icc.tag_count as usize].to_vec();
    let orig_sizes: Vec<u32> = icc.tag_sizes[..icc.tag_count as usize].to_vec();

    // Pass 1: NULL io computes offsets + sizes
    let mut null_io = io::io_handler_from_null(icc.ctx.as_ref());
    if !write_header(icc, 0, &mut null_io)
        || !save_tags(
            icc,
            &mut null_io,
            orig_io.as_mut(),
            &orig_offsets,
            &orig_sizes,
        )
    {
        restore_dir(icc, &orig_offsets, &orig_sizes);
        icc.io = orig_io;
        return 0;
    }
    let used_space = null_io.used_space;

    // Pass 2: real write into the target handler
    if let Some(out_io) = out {
        if !set_links(icc)
            || !write_header(icc, used_space, out_io)
            || !save_tags(icc, out_io, orig_io.as_mut(), &orig_offsets, &orig_sizes)
        {
            restore_dir(icc, &orig_offsets, &orig_sizes);
            icc.io = orig_io;
            return 0;
        }
    }
    // C restores the entire Keep copy — offsets/sizes must return to
    // their pre-call values for subsequent save calls.
    restore_dir(icc, &orig_offsets, &orig_sizes);
    icc.io = orig_io;
    used_space
}

fn restore_dir(icc: &mut IccProfile, offsets: &[u32], sizes: &[u32]) {
    icc.tag_offsets[..offsets.len()].copy_from_slice(offsets);
    icc.tag_sizes[..sizes.len()].copy_from_slice(sizes);
}

/// `cmsSaveProfileToMem` — sizing pass, then a sized buffer write
pub fn save_profile_to_mem(icc: &mut IccProfile) -> Option<Vec<u8>> {
    let needed = save_profile_to_iohandler(icc, None);
    if needed == 0 {
        return None;
    }
    let mut mem = io::io_handler_from_mem(None, vec![0u8; needed as usize], "w")?;
    if save_profile_to_iohandler(icc, Some(&mut mem)) == 0 {
        return None;
    }
    match mem.stream {
        io::IoStream::Mem(m) => Some(m.block[..needed as usize].to_vec()),
        _ => None,
    }
}

/// `cmsSaveProfileToFile`
pub fn save_profile_to_file(icc: &mut IccProfile, path: &str) -> bool {
    let Some(mut io) = io::io_handler_from_file(None, path, "w") else {
        return false;
    };
    let rc = save_profile_to_iohandler(icc, Some(&mut io)) != 0;
    if !rc {
        let _ = std::fs::remove_file(path);
    }
    rc
}

/// `cmsOpenProfileFromFile` — `access` like "r"/"w"
pub fn open_profile_from_file(
    ctx: Option<Context>,
    path: &str,
    access: &str,
) -> Option<Box<IccProfile>> {
    let io = io::io_handler_from_file(None, path, access)?;
    let mut icc = Box::new(IccProfile {
        io: Some(io),
        ctx,
        ..Default::default()
    });
    if access.starts_with('r') && !read_header(&mut icc) {
        return None;
    }
    Some(icc)
}

// ---------------------------------------------------------------------------
// Header getters/setters (cmsio0.c 1141-1330)
// ---------------------------------------------------------------------------

/// `cmsGetProfileVersion` — float version (BCD-decoded)
pub fn get_profile_version(icc: &IccProfile) -> f64 {
    let n = icc.version >> 16;
    format!("{n:x}").parse::<f64>().unwrap_or(0.0) / 100.0
}

/// `cmsSetProfileVersion` — write a float version back as encoded u32
pub fn set_profile_version(icc: &mut IccProfile, ver: f64) {
    let major = ver as u32;
    let minor = ((ver - major as f64) * 100.0).round() as u32;
    icc.version = (u32::from_str_radix(&format!("{major}{minor:02}"), 16).unwrap_or(0)) << 16;
}

/// `cmsGetEncodedICCversion` / `cmsSetEncodedICCversion`
pub fn get_encoded_icc_version(icc: &IccProfile) -> u32 {
    icc.version
}
pub fn set_encoded_icc_version(icc: &mut IccProfile, v: u32) {
    icc.version = v;
}
