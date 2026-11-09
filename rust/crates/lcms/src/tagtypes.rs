//! Port of `cmstypes.c` — ICC tag type handlers.
//!
//! C source: `reference/lcms2/src/cmstypes.c`
//! Gate: `tests/c_parity_tagtypes.rs`
//!
//! C's handlers are `ReadPtr/WritePtr/DupPtr/FreePtr` on `void*` payloads.
//! Here the payload is the `TagObject` enum; `dup` is `clone`, `free` is `drop`.
//! Every Read fn returns the right enum variant with `*nItems` semantics
//! preserved via the return tuple.

use crate::context::Context;
use crate::err;
use crate::fixed::{double_to_s15_fixed16, double_to_u8_fixed8, u8_fixed8_to_double};
use crate::gamma::{
    CurveSegment, ToneCurve, build_parametric_tone_curve, build_segmented_tone_curve,
    build_tabulated_tone_curve16,
};
use crate::io::{self, IoHandler};
use crate::named::{Mlu, Wchar, mlu_get_ascii, mlu_get_wide, mlu_set_ascii, mlu_set_wide};
use crate::signatures::tagtype as tsig;
use crate::types::{CpowExt, *};

/// `cmsNoLanguage`/`cmsNoCountry` — two NUL bytes
pub const NO_LANG: &[u8] = b"\0\0";
/// `cmsV2Unicode` — 0xffff marker used by textDescription unicode field
pub const V2_UNICODE: &[u8] = b"\xff\xff";

const CMS_MAX_CHANNELS: usize = 16;

/// `cmsICCData` — data type payload
#[derive(Clone)]
pub struct IccData {
    pub len: u32,
    pub flag: u32,
    pub data: Vec<u8>,
}

/// `struct tm` equivalent
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
}

/// `cmsScreeningChannel`
#[derive(Clone, Copy, Debug, Default)]
pub struct ScreeningChannel {
    pub frequency: f64,
    pub angle: f64,
    pub spot_shape: u32,
}

/// `cmsScreening`
#[derive(Clone)]
pub struct Screening {
    pub flag: u32,
    pub channels: [ScreeningChannel; CMS_MAX_CHANNELS],
    pub n_channels: u32,
}

/// `cmsICCViewingConditions` (ICC viewingConditionsType)
#[derive(Clone)]
pub struct IccViewingConditions {
    pub illuminant_xyz: CieXyz,
    pub surround_xyz: CieXyz,
    pub illuminant_type: u32,
}

/// `cmsUcrBg`
#[derive(Clone)]
pub struct UcrBg {
    pub ucr: ToneCurve,
    pub bg: ToneCurve,
    pub desc: Mlu,
}

/// `cmsVideoSignalType`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VideoSignal {
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub video_full_range_flag: u8,
}

/// `cmsMHC2Type`
#[derive(Clone)]
pub struct Mhc2 {
    pub curve_entries: u32,
    pub red_curve: Vec<f64>,
    pub green_curve: Vec<f64>,
    pub blue_curve: Vec<f64>,
    pub min_luminance: f64,
    pub peak_luminance: f64,
    /// `XYZ2XYZmatrix[3][4]`
    pub xyz2xyz_matrix: [[f64; 4]; 3],
}

/// `cmsSigCrdInfoType` payload is a `cmsMLU*`
#[derive(Clone)]
pub struct CrdInfo {
    pub product_name: Mlu,
}

/// The `void*` payload of a cooked tag — one variant per handler family.
#[derive(Clone)]
pub enum TagObject {
    Xyz(CieXyz),
    Chromaticity(CiexyYTriple),
    ColorantOrder([u8; CMS_MAX_CHANNELS]),
    UInt8(Vec<u8>),
    UInt32(Vec<u32>),
    UInt64(Vec<u64>),
    S15Fixed16(Vec<f64>),
    U16Fixed16(Vec<f64>),
    Signature(u32),
    Text(Mlu),
    TextDescription(Mlu),
    Data(IccData),
    Curve(ToneCurve),
    ParametricCurve(ToneCurve),
    DateTime(Tm),
    Measurement(IccMeasurementConditions),
    Mlu(Mlu),
    Lut8(crate::lut::Pipeline),
    Lut16(crate::lut::Pipeline),
    LutA2B(crate::lut::Pipeline),
    LutB2A(crate::lut::Pipeline),
    Mpe(crate::lut::Pipeline),
    /// single `cmsStage*` — payload for MPE element handlers
    MpeElem(crate::lut::Stage),
    ColorantTable(crate::named::NamedColorList),
    NamedColor(crate::named::NamedColorList),
    ProfileSeqDesc(crate::named::ProfileSeqDesc),
    ProfileSeqId(crate::named::ProfileSeqDesc),
    UcrBg(UcrBg),
    CrdInfo(Mlu),
    Screening(Screening),
    ViewingConditions(IccViewingConditions),
    Dictionary(crate::named::Dict),
    VideoSignal(VideoSignal),
    Vcgt(VcgtData),
    Mhc2(Mhc2),
    /// A tag object owned by a C-registered `cmsTagTypeHandler` — the raw
    /// `void*` from `ReadPtr`/`DupPtr`. `cbox` is the FFI-side C-ABI handler
    /// supplying `FreePtr`/`DupPtr`/`ContextID`.
    Foreign(ForeignTag),
    Raw(Vec<u8>),
}

/// The payload of `TagObject::Foreign` — raw `void*` + owning C handler.
pub struct ForeignTag {
    pub ptr: usize,
    pub cbox: usize,
}

impl Clone for ForeignTag {
    fn clone(&self) -> Self {
        // `DupPtr(self, Ptr, n)` — a fresh owned copy, like C
        if let Some(h) = *crate::context::TAGTYPE_HOOKS.read().unwrap() {
            let dup = (h.dup)(self.cbox, self.ptr, 1);
            if dup != 0 {
                return ForeignTag { ptr: dup, cbox: self.cbox };
            }
        }
        ForeignTag { ptr: self.ptr, cbox: self.cbox }
    }
}

impl Drop for ForeignTag {
    fn drop(&mut self) {
        // `FreePtr(self, Ptr)`
        if self.ptr != 0 {
            if let Some(h) = *crate::context::TAGTYPE_HOOKS.read().unwrap() {
                (h.free)(self.cbox, self.ptr);
            }
        }
    }
}

/// vcgt payload — either 3 curves (formula/table) or a raw table
#[derive(Clone)]
pub enum VcgtData {
    Curves([ToneCurve; 3]),
    Table {
        channels: u16,
        elems: u16,
        bytes: u16,
        data: Vec<u8>,
    },
}

/// `cmsTagTypeHandler` — read/write/dup function table. `FreePtr`/`DupPtr`
/// are absorbed by `Clone`/`Drop` on `TagObject`.
#[derive(Clone, Copy)]
pub struct TagTypeHandler {
    pub signature: u32,
    pub read: fn(&TagTypeHandler, &mut IoHandler, &mut u32, u32) -> Option<TagObject>,
    pub write: fn(&TagTypeHandler, &mut IoHandler, &TagObject, u32) -> bool,
    // `self` copies like LocalTypeHandler get these stamped in
    pub icc_version: u32,
    /// nonzero → a C-registered handler; the value is the FFI's C-ABI
    /// `cmsTagTypeHandler` box passed as `self` to the C callbacks.
    pub foreign: usize,
    /// stamped `*const Context` — C's `self->ContextID`; lets nested
    /// dispatch (MPE element handlers) reach the plugin lists.
    pub ctx: usize,
}

/// Foreign `ReadPtr` — invoke the C callback through `TAGTYPE_HOOKS` and wrap
/// the returned `void*` as `TagObject::Foreign`.
fn foreign_read(
    h: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size: u32,
) -> Option<TagObject> {
    let hooks = (*crate::context::TAGTYPE_HOOKS.read().unwrap())?;
    let ptr = (hooks.read)(
        h.foreign,
        h.icc_version,
        io as *mut IoHandler as usize,
        n_items as *mut u32 as usize,
        size as usize,
    );
    if ptr == 0 {
        None
    } else {
        Some(TagObject::Foreign(ForeignTag {
            ptr,
            cbox: h.foreign,
        }))
    }
}

/// Foreign `WritePtr` — pass the `Foreign` payload's raw pointer to the C
/// callback.
fn foreign_write(h: &TagTypeHandler, io: &mut IoHandler, obj: &TagObject, n: u32) -> bool {
    let Some(hooks) = *crate::context::TAGTYPE_HOOKS.read().unwrap() else {
        return false;
    };
    let TagObject::Foreign(f) = obj else {
        return false;
    };
    (hooks.write)(
        h.foreign,
        h.icc_version,
        io as *mut IoHandler as usize,
        f.ptr,
        n as usize,
    )
}

/// A `TagTypeHandler` whose callbacks bridge into a C-registered
/// `cmsTagTypeHandler` (`cbox` = FFI handler struct pointer).
pub fn foreign_tag_type_handler(signature: u32, cbox: usize) -> TagTypeHandler {
    TagTypeHandler {
        signature,
        read: foreign_read,
        write: foreign_write,
        icc_version: 0,
        foreign: cbox,
        ctx: 0,
    }
}

// ---------------------------------------------------------------------------
// wchar helpers (UTF-16 ↔ UTF-32 on this platform)
// ---------------------------------------------------------------------------

/// `_cmsWriteWCharArray` — wchar (UTF-32) → UTF-16, non-BMP → '?'
pub fn write_wchar_array(io: &mut IoHandler, array: &[Wchar]) -> bool {
    // NOTE: upstream's `#if (WCHAR_MAX > 0xFFFF)` evaluates false in this
    // TU because only <limits.h> is included (WCHAR_MAX lives in <stdint.h>),
    // so the compiled behavior is the UTF-16 branch: verbatim truncation of
    // the 32-bit wchar to u16 — negatives and astral chars included.
    for &v in array {
        let out: u16 = v as u16;
        if !io::write_u16(io, out) {
            return false;
        }
    }
    true
}

#[inline]
fn is_surrogate(uc: u32) -> bool {
    uc.wrapping_sub(0xd800) < 2048
}
#[inline]
fn is_high_surrogate(uc: u32) -> bool {
    (uc & 0xfffffc00) == 0xd800
}
#[inline]
fn is_low_surrogate(uc: u32) -> bool {
    (uc & 0xfffffc00) == 0xdc00
}
#[inline]
fn surrogate_to_utf32(high: u32, low: u32) -> u32 {
    (high << 10) + low - 0x35fdc00
}

/// `convert_utf16_to_utf32`
fn convert_utf16_to_utf32(io: &mut IoHandler, n: i32, output: &mut Vec<Wchar>) -> bool {
    let mut n = n;
    while n > 0 {
        let mut uc = 0u16;
        if !io::read_u16(io, &mut uc) {
            return false;
        }
        n -= 1;
        if !is_surrogate(uc as u32) {
            output.push(uc as Wchar);
        } else {
            let mut low = 0u16;
            if !io::read_u16(io, &mut low) {
                return false;
            }
            n -= 1;
            if is_high_surrogate(uc as u32) && is_low_surrogate(low as u32) {
                output.push(surrogate_to_utf32(uc as u32, low as u32) as Wchar);
            } else {
                return false;
            }
        }
    }
    true
}

/// `_cmsReadWCharArray` — UTF-16 → UTF-32 (surrogates joined)
pub fn read_wchar_array(io: &mut IoHandler, n: u32, array: &mut Vec<Wchar>) -> bool {
    convert_utf16_to_utf32(io, n as i32, array)
}

// ---------------------------------------------------------------------------
// Position tables
// ---------------------------------------------------------------------------

pub type PositionTableEntryFn = fn(&TagTypeHandler, &mut IoHandler, usize, u32) -> bool;

/// `ReadPositionTable` — read Count (offset,size) records then each element.
/// `elem` is a closure: (io, index, element_size) -> bool
pub fn read_position_table(
    io: &mut IoHandler,
    count: u32,
    base_offset: u32,
    mut elem: impl FnMut(&mut IoHandler, usize, u32) -> bool,
) -> bool {
    let current = io.tell();
    if (io.reported_size.saturating_sub(current)) / 8 < count {
        return false;
    }

    let mut offsets = vec![0u32; count as usize];
    let mut sizes = vec![0u32; count as usize];
    for i in 0..count as usize {
        if !io::read_u32(io, &mut offsets[i]) || !io::read_u32(io, &mut sizes[i]) {
            return false;
        }
        offsets[i] = offsets[i].wrapping_add(base_offset);
    }

    for i in 0..count as usize {
        if !io.seek(offsets[i]) || !elem(io, i, sizes[i]) {
            return false;
        }
    }
    true
}

/// `WritePositionTable` — reserve the directory, write elements, backfill.
/// `elem` is a closure: (io, index, size_of_tag) -> bool
pub fn write_position_table(
    io: &mut IoHandler,
    size_of_tag: u32,
    count: u32,
    base_offset: u32,
    mut elem: impl FnMut(&mut IoHandler, usize, u32) -> bool,
) -> bool {
    let mut offsets = vec![0u32; count as usize];
    let mut sizes = vec![0u32; count as usize];

    let dir_pos = io.tell();

    for _ in 0..count {
        if !io::write_u32(io, 0) || !io::write_u32(io, 0) {
            return false;
        }
    }

    for i in 0..count as usize {
        let before = io.tell();
        offsets[i] = before.wrapping_sub(base_offset);
        if !elem(io, i, size_of_tag) {
            return false;
        }
        sizes[i] = io.tell() - before;
    }

    let current = io.tell();
    if !io.seek(dir_pos) {
        return false;
    }
    for i in 0..count as usize {
        if !io::write_u32(io, offsets[i]) || !io::write_u32(io, sizes[i]) {
            return false;
        }
    }
    io.seek(current)
}

// ---------------------------------------------------------------------------
// Leaf handlers
// ---------------------------------------------------------------------------

/// `Type_XYZ_Read` — single XYZ
fn type_xyz_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut xyz = CieXyz::default();
    if !io::read_xyz(io, &mut xyz) {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Xyz(xyz))
}

fn type_xyz_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Xyz(xyz) = p else { return false };
    io::write_xyz(io, xyz)
}

/// `DecideXYZtype`
pub fn decide_xyz_type(_ver: f64, data: &TagObject) -> u32 {
    // Reject type-mismatched payloads — C's tag-descriptor table
    // validates the object against the sig's allowed types.
    if !matches!(data, TagObject::Xyz(_)) { return 0 }
    tsig::SigXYZType
}

/// `Type_Chromaticity_Read`
fn type_chromaticity_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut chrm = CiexyYTriple::default();
    let mut n_chans = 0u16;
    if !io::read_u16(io, &mut n_chans) {
        return None;
    }
    // recover from a bug in early lcms1
    if n_chans == 0 && size_of_tag == 32 {
        let mut dummy = 0u16;
        if !io::read_u16(io, &mut dummy) || !io::read_u16(io, &mut n_chans) {
            return None;
        }
    }
    if n_chans != 3 {
        return None;
    }
    let mut table = 0u16;
    if !io::read_u16(io, &mut table) {
        return None;
    }
    for ch in [&mut chrm.Red, &mut chrm.Green, &mut chrm.Blue] {
        if !io::read_15_fixed16(io, &mut ch.x) || !io::read_15_fixed16(io, &mut ch.y) {
            return None;
        }
        ch.Y = 1.0;
    }
    *n_items = 1;
    Some(TagObject::Chromaticity(chrm))
}

fn save_one_chromaticity(x: f64, y: f64, io: &mut IoHandler) -> bool {
    io::write_u32(io, double_to_s15_fixed16(x) as u32)
        && io::write_u32(io, double_to_s15_fixed16(y) as u32)
}

fn type_chromaticity_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let TagObject::Chromaticity(chrm) = p else {
        return false;
    };
    if !io::write_u16(io, 3) || !io::write_u16(io, 0) {
        return false;
    }
    save_one_chromaticity(chrm.Red.x, chrm.Red.y, io)
        && save_one_chromaticity(chrm.Green.x, chrm.Green.y, io)
        && save_one_chromaticity(chrm.Blue.x, chrm.Blue.y, io)
}

/// `Type_ColorantOrderType_Read`
fn type_colorant_order_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    if count > CMS_MAX_CHANNELS as u32 {
        return None;
    }
    let mut order = [0xFFu8; CMS_MAX_CHANNELS];
    if io.read(&mut order, 1, count) != count {
        return None;
    }
    *n_items = 1;
    Some(TagObject::ColorantOrder(order))
}

fn type_colorant_order_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let TagObject::ColorantOrder(order) = p else {
        return false;
    };
    let count = order.iter().filter(|&&b| b != 0xFF).count() as u32;
    if !io::write_u32(io, count) {
        return false;
    }
    io.write(count, &order[..count as usize])
}

macro_rules! int_array_handler {
    ($read:ident, $write:ident, $variant:ident, $elem:ty, $rd:expr, $wr:expr) => {
        fn $read(
            _s: &TagTypeHandler,
            io: &mut IoHandler,
            n_items: &mut u32,
            size_of_tag: u32,
        ) -> Option<TagObject> {
            *n_items = 0;
            let n = size_of_tag / std::mem::size_of::<$elem>() as u32;
            let mut arr = vec![0 as $elem; n as usize];
            let rd: fn(&mut IoHandler, &mut $elem) -> bool = $rd;
            for v in &mut arr {
                if !rd(io, v) {
                    return None;
                }
            }
            *n_items = n;
            Some(TagObject::$variant(arr))
        }
        fn $write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, n_items: u32) -> bool {
            let TagObject::$variant(arr) = p else {
                return false;
            };
            let wr: fn(&mut IoHandler, $elem) -> bool = $wr;
            for &v in arr.iter().take(n_items as usize) {
                if !wr(io, v) {
                    return false;
                }
            }
            true
        }
    };
}

int_array_handler!(
    type_u8_read,
    type_u8_write,
    UInt8,
    u8,
    |i, n| io::read_u8(i, n),
    |i, v| io::write_u8(i, v)
);
int_array_handler!(
    type_u32_read,
    type_u32_write,
    UInt32,
    u32,
    |i, n| io::read_u32(i, n),
    |i, v| io::write_u32(i, v)
);
int_array_handler!(
    type_u64_read,
    type_u64_write,
    UInt64,
    u64,
    |i, n| io::read_u64(i, n),
    |i, v| io::write_u64(i, v)
);

/// `Type_S15Fixed16_Read`
fn type_s15_fixed16_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let n = size_of_tag / 4;
    let mut arr = vec![0.0f64; n as usize];
    for v in &mut arr {
        if !io::read_15_fixed16(io, v) {
            return None;
        }
    }
    *n_items = n;
    Some(TagObject::S15Fixed16(arr))
}

fn type_s15_fixed16_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    n_items: u32,
) -> bool {
    let TagObject::S15Fixed16(arr) = p else {
        return false;
    };
    for &v in arr.iter().take(n_items as usize) {
        if !io::write_15_fixed16(io, v) {
            return false;
        }
    }
    true
}

/// `Type_U16Fixed16_Read`
fn type_u16_fixed16_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let n = size_of_tag / 4;
    let mut arr = vec![0.0f64; n as usize];
    for v in &mut arr {
        let mut raw = 0u32;
        if !io::read_u32(io, &mut raw) {
            return None;
        }
        *v = raw as f64 / 65536.0;
    }
    *n_items = n;
    Some(TagObject::U16Fixed16(arr))
}

fn type_u16_fixed16_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    n_items: u32,
) -> bool {
    let TagObject::U16Fixed16(arr) = p else {
        return false;
    };
    for &v in arr.iter().take(n_items as usize) {
        let w = (v * 65536.0 + 0.5).floor() as u32;
        if !io::write_u32(io, w) {
            return false;
        }
    }
    true
}

/// `Type_Signature_Read`
fn type_signature_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    let mut sig = 0u32;
    if !io::read_u32(io, &mut sig) {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Signature(sig))
}

fn type_signature_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Signature(sig) = p else {
        return false;
    };
    io::write_u32(io, *sig)
}

/// `Type_Text_Read` — reads a textType into an MLU
fn type_text_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut mlu = crate::named::mlu_alloc(None, 1)?;
    if size_of_tag == u32::MAX {
        return None;
    }
    let mut text = vec![0u8; size_of_tag as usize + 1];
    if io.read(&mut text, 1, size_of_tag) != size_of_tag {
        return None;
    }
    // Text[SizeOfTag] = 0 already; truncate at first NUL like strlen
    let strlen = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    if !mlu_set_ascii(&mut mlu, NO_LANG, NO_LANG, &text[..strlen]) {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Text(mlu))
}

fn type_text_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    // C: every text-family handler takes the same cmsMLU*
    let (TagObject::Text(mlu) | TagObject::TextDescription(mlu) | TagObject::Mlu(mlu)) = p else {
        return false;
    };
    let (size, _) = mlu_get_ascii(mlu, NO_LANG, NO_LANG, None);
    if size == 0 {
        return false;
    }
    let (_, text) = mlu_get_ascii(mlu, NO_LANG, NO_LANG, Some(size));
    io.write(size, &text[..size as usize])
}

/// `DecideTextType`
pub fn decide_text_type(ver: f64, data: &TagObject) -> u32 {
    if !matches!(data,
        TagObject::Mlu(_) | TagObject::Text(_) | TagObject::TextDescription(_))
    {
        return 0;
    }
    if ver >= 4.0 {
        tsig::SigMultiLocalizedUnicodeType
    } else {
        tsig::SigTextType
    }
}

/// `Type_Data_Read` — cmsICCData
fn type_data_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    if size_of_tag < 4 {
        return None;
    }
    let len = size_of_tag - 4;
    if len > i32::MAX as u32 {
        return None;
    }
    let mut flag = 0u32;
    if !io::read_u32(io, &mut flag) {
        return None;
    }
    let mut data = vec![0u8; len as usize];
    if io.read(&mut data, 1, len) != len {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Data(IccData { len, flag, data }))
}

fn type_data_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Data(d) = p else { return false };
    io::write_u32(io, d.flag) && io.write(d.len, &d.data)
}

/// `Type_Text_Description_Read` — 'desc' type into an MLU
fn type_text_description_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut size_of_tag = size_of_tag;

    if size_of_tag < 4 {
        return None;
    }
    let mut ascii_count = 0u32;
    if !io::read_u32(io, &mut ascii_count) {
        return None;
    }
    if ascii_count > 0x7ffff {
        return None;
    }
    size_of_tag -= 4;
    if size_of_tag < ascii_count {
        return None;
    }

    let mut mlu = crate::named::mlu_alloc(None, 2)?;
    let mut text = vec![0u8; ascii_count as usize + 1];
    if io.read(&mut text, 1, ascii_count) != ascii_count {
        return None;
    }
    size_of_tag -= ascii_count;
    let strlen = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    if !mlu_set_ascii(&mut mlu, NO_LANG, NO_LANG, &text[..strlen]) {
        return None;
    }

    // Unicode block — tolerant: failures just stop (goto Done)
    'done: {
        if size_of_tag < 8 {
            break 'done;
        }
        let mut unicode_code = 0u32;
        let mut unicode_count = 0u32;
        if !io::read_u32(io, &mut unicode_code) || !io::read_u32(io, &mut unicode_count) {
            break 'done;
        }
        size_of_tag -= 8;
        if unicode_count == 0 || unicode_count > 0x7ffff || size_of_tag < unicode_count * 2 {
            break 'done;
        }
        let mut uni = Vec::new();
        if !read_wchar_array(io, unicode_count, &mut uni) {
            break 'done;
        }
        uni.push(0);
        if !mlu_set_wide(&mut mlu, V2_UNICODE, V2_UNICODE, &uni) {
            break 'done;
        }
        size_of_tag -= unicode_count * 2;

        // ScriptCode if present
        if size_of_tag >= 2 + 1 + 67 {
            let mut sc_code = 0u16;
            let mut sc_count = 0u8;
            if !io::read_u16(io, &mut sc_code) || !io::read_u8(io, &mut sc_count) {
                break 'done;
            }
            let mut dummy = [0u8; 67];
            if io.read(&mut dummy, 1, 67) != 67 {
                return None;
            }
        }
    }

    *n_items = 1;
    Some(TagObject::TextDescription(mlu))
}

/// `Type_Text_Description_Write` — 'desc' with forced zero alignment
fn type_text_description_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let (TagObject::TextDescription(mlu) | TagObject::Text(mlu) | TagObject::Mlu(mlu)) = p else {
        return false;
    };
    let filler = [0u8; 68];

    let len = mlu_get_ascii(mlu, NO_LANG, NO_LANG, None).0;
    let (text, wide) = if len == 0 {
        (b"\0".to_vec(), vec![0i32])
    } else {
        let (_, t) = mlu_get_ascii(mlu, NO_LANG, NO_LANG, Some(len));
        let (_, w) = mlu_get_wide(mlu, V2_UNICODE, V2_UNICODE, Some(len * 4));
        (t, w)
    };

    let strlen = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let len_text = strlen as u32 + 1;
    let len_req = 8 + 4 + len_text + 4 + 4 + 2 * len_text + 2 + 1 + 67;
    let len_aligned = io::align_long(len_req);

    if !io::write_u32(io, len_text) || !io.write(len_text, &text[..len_text as usize]) {
        return false;
    }
    if !io::write_u32(io, 0) {
        return false;
    }
    if !io::write_u32(io, len_text) {
        return false;
    }
    // write the wide chars — C writes len_text wchars incl the NUL
    let wide_sized = {
        let mut w = wide;
        w.resize(len_text as usize, 0);
        w
    };
    if !write_wchar_array(io, &wide_sized) {
        return false;
    }
    if !io::write_u16(io, 0) || !io::write_u8(io, 0) || !io.write(67, &filler[..67]) {
        return false;
    }
    if len_aligned > len_req {
        let pad = len_aligned - len_req;
        if !io.write(pad, &filler[..pad as usize]) {
            return false;
        }
    }
    true
}

/// `DecideTextDescType`
pub fn decide_text_desc_type(ver: f64, data: &TagObject) -> u32 {
    // Type-mismatched payloads must be rejected — C's tag-descriptor
    // table validates the object's type against the sig's allowed
    // types; a non-text payload here returns 0 so is_type_supported
    // fails and cms_write_tag rejects the write.
    if !matches!(data,
        TagObject::Mlu(_) | TagObject::Text(_) | TagObject::TextDescription(_))
    {
        return 0;
    }
    if ver >= 4.0 {
        tsig::SigMultiLocalizedUnicodeType
    } else {
        tsig::SigTextDescriptionType
    }
}

/// `Type_Curve_Read` — curveType (0 = linear, 1 = u8Fixed8 gamma, n = table)
fn type_curve_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    match count {
        0 => {
            // Linear
            let g = build_parametric_tone_curve(None, 1, &[1.0])?;
            *n_items = 1;
            Some(TagObject::Curve(g))
        }
        1 => {
            let mut g_fixed = 0u16;
            if !io::read_u16(io, &mut g_fixed) {
                return None;
            }
            let single = u8_fixed8_to_double(g_fixed);
            *n_items = 1;
            Some(TagObject::Curve(build_parametric_tone_curve(
                None,
                1,
                &[single],
            )?))
        }
        _ => {
            if count > 0x7FFF {
                return None;
            }
            let mut g = build_tabulated_tone_curve16(None, count, None)?;
            if !io::read_u16_array(io, count, Some(&mut g.table16)) {
                return None;
            }
            *n_items = 1;
            Some(TagObject::Curve(g))
        }
    }
}

fn type_curve_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Curve(curve) = p else {
        return false;
    };
    if curve.segments.len() == 1 && curve.segments[0].r#type == 1 {
        let g_fixed = double_to_u8_fixed8(curve.segments[0].params[0]);
        return io::write_u32(io, 1) && io::write_u16(io, g_fixed);
    }
    if !io::write_u32(io, curve.table16.len() as u32) {
        return false;
    }
    io::write_u16_array(io, &curve.table16)
}

/// `DecideCurveType`
pub fn decide_curve_type(ver: f64, data: &TagObject) -> u32 {
    let (TagObject::Curve(c) | TagObject::ParametricCurve(c)) = data else {
        return 0; // type-mismatched payload — is_type_supported rejects
    };
    if ver < 4.0 || c.segments.len() != 1 || c.segments[0].r#type < 0 || c.segments[0].r#type > 5 {
        tsig::SigCurveType
    } else {
        tsig::SigParametricCurveType
    }
}

/// `Type_ParametricCurve_Read`
fn type_parametric_curve_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    const PARAMS_BY_TYPE: [i32; 5] = [1, 3, 4, 5, 7];
    *n_items = 0;
    let mut typ = 0u16;
    if !io::read_u16(io, &mut typ) {
        return None;
    }
    let mut reserved = 0u16;
    if !io::read_u16(io, &mut reserved) {
        return None;
    }
    if typ > 4 {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "Unknown parametric curve type",
        );
        return None;
    }
    let mut params = [0.0f64; 10];
    let n = PARAMS_BY_TYPE[typ as usize];
    for p in params.iter_mut().take(n as usize) {
        if !io::read_15_fixed16(io, p) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::ParametricCurve(build_parametric_tone_curve(
        None,
        typ as i32 + 1,
        &params,
    )?))
}

fn type_parametric_curve_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    const PARAMS_BY_TYPE: [i32; 6] = [0, 1, 3, 4, 5, 7];
    let (TagObject::ParametricCurve(curve) | TagObject::Curve(curve)) = p else {
        return false;
    };
    let typen = curve.segments[0].r#type;
    if curve.segments.len() > 1 || typen < 1 {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "Multisegment or Inverted parametric curves cannot be written",
        );
        return false;
    }
    if typen > 5 {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "Unsupported parametric curve",
        );
        return false;
    }
    let n_params = PARAMS_BY_TYPE[typen as usize];
    if !io::write_u16(io, (typen - 1) as u16) || !io::write_u16(io, 0) {
        return false;
    }
    for i in 0..n_params as usize {
        if !io::write_15_fixed16(io, curve.segments[0].params[i]) {
            return false;
        }
    }
    true
}

/// `Type_DateTime_Read` — 12-byte dateTime → Tm
fn type_datetime_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut buf = [0u8; 12];
    if io.read(&mut buf, 12, 1) != 1 {
        return None;
    }
    let d = DateTimeNumber {
        year: io::adjust_endian16(u16::from_ne_bytes(buf[0..2].try_into().unwrap())),
        month: io::adjust_endian16(u16::from_ne_bytes(buf[2..4].try_into().unwrap())),
        day: io::adjust_endian16(u16::from_ne_bytes(buf[4..6].try_into().unwrap())),
        hours: io::adjust_endian16(u16::from_ne_bytes(buf[6..8].try_into().unwrap())),
        minutes: io::adjust_endian16(u16::from_ne_bytes(buf[8..10].try_into().unwrap())),
        seconds: io::adjust_endian16(u16::from_ne_bytes(buf[10..12].try_into().unwrap())),
    };
    *n_items = 1;
    Some(TagObject::DateTime(Tm {
        tm_sec: d.seconds as i32,
        tm_min: d.minutes as i32,
        tm_hour: d.hours as i32,
        tm_mday: d.day as i32,
        tm_mon: d.month as i32 - 1,
        tm_year: d.year as i32 - 1900,
        tm_wday: -1,
        tm_yday: -1,
        tm_isdst: 0,
    }))
}

fn type_datetime_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::DateTime(tm) = p else {
        return false;
    };
    let d = DateTimeNumber {
        year: (tm.tm_year + 1900) as u16,
        month: (tm.tm_mon + 1) as u16,
        day: tm.tm_mday as u16,
        hours: tm.tm_hour as u16,
        minutes: tm.tm_min as u16,
        seconds: tm.tm_sec as u16,
    };
    let mut buf = [0u8; 12];
    for (i, v) in [d.year, d.month, d.day, d.hours, d.minutes, d.seconds]
        .iter()
        .enumerate()
    {
        buf[i * 2..i * 2 + 2].copy_from_slice(&io::adjust_endian16(*v).to_ne_bytes());
    }
    io.write(12, &buf)
}

/// `Type_Measurement_Read`/`Write`
fn type_measurement_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut mc = IccMeasurementConditions::default();
    let mut backing = CieXyz::default();
    if !io::read_u32(io, &mut mc.observer)
        || !io::read_xyz(io, &mut backing)
        || !io::read_u32(io, &mut mc.geometry)
        || !io::read_15_fixed16(io, &mut mc.flare)
        || !io::read_u32(io, &mut mc.illuminant)
    {
        return None;
    }
    mc.backing = EncodedXyzNumber {
        X: double_to_s15_fixed16(backing.X),
        Y: double_to_s15_fixed16(backing.Y),
        Z: double_to_s15_fixed16(backing.Z),
    };
    // C keeps the struct as stored — Backing is cmsCIEXYZ (decoded doubles)
    *n_items = 1;
    Some(TagObject::Measurement(mc))
}

fn type_measurement_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Measurement(mc) = p else {
        return false;
    };
    io::write_u32(io, mc.observer)
        && io::write_xyz(
            io,
            &CieXyz {
                X: crate::fixed::s15_fixed16_to_double(mc.backing.X),
                Y: crate::fixed::s15_fixed16_to_double(mc.backing.Y),
                Z: crate::fixed::s15_fixed16_to_double(mc.backing.Z),
            },
        )
        && io::write_u32(io, mc.geometry)
        && io::write_15_fixed16(io, mc.flare)
        && io::write_u32(io, mc.illuminant)
}

// ---------------------------------------------------------------------------
// The type-handler table (`SupportedTagTypes` — plugin list prepends)
// ---------------------------------------------------------------------------

macro_rules! handler {
    ($sig:expr, $read:ident, $write:ident) => {
        TagTypeHandler {
            signature: $sig,
            read: $read,
            write: $write,
            icc_version: 0,
            foreign: 0,
            ctx: 0,
        }
    };
}

/// `SupportedTagTypes` — the built-in list, in C's exact order.
pub static SUPPORTED_TAG_TYPES: &[TagTypeHandler] = &[
    handler!(
        tsig::SigChromaticityType,
        type_chromaticity_read,
        type_chromaticity_write
    ),
    handler!(
        tsig::SigColorantOrderType,
        type_colorant_order_read,
        type_colorant_order_write
    ),
    handler!(
        tsig::SigS15Fixed16ArrayType,
        type_s15_fixed16_read,
        type_s15_fixed16_write
    ),
    handler!(
        tsig::SigU16Fixed16ArrayType,
        type_u16_fixed16_read,
        type_u16_fixed16_write
    ),
    handler!(tsig::SigTextType, type_text_read, type_text_write),
    handler!(
        tsig::SigTextDescriptionType,
        type_text_description_read,
        type_text_description_write
    ),
    handler!(tsig::SigCurveType, type_curve_read, type_curve_write),
    handler!(
        tsig::SigParametricCurveType,
        type_parametric_curve_read,
        type_parametric_curve_write
    ),
    handler!(
        tsig::SigDateTimeType,
        type_datetime_read,
        type_datetime_write
    ),
    handler!(tsig::SigLut8Type, type_lut8_read, type_lut8_write),
    handler!(tsig::SigLut16Type, type_lut16_read, type_lut16_write),
    handler!(
        tsig::SigColorantTableType,
        type_colorant_table_read,
        type_colorant_table_write
    ),
    handler!(
        tsig::SigNamedColor2Type,
        type_named_color_read,
        type_named_color_write
    ),
    handler!(
        tsig::SigMultiLocalizedUnicodeType,
        type_mlu_read,
        type_mlu_write
    ),
    handler!(
        tsig::SigProfileSequenceDescType,
        type_profile_seq_desc_read,
        type_profile_seq_desc_write
    ),
    handler!(
        tsig::SigSignatureType,
        type_signature_read,
        type_signature_write
    ),
    handler!(
        tsig::SigMeasurementType,
        type_measurement_read,
        type_measurement_write
    ),
    handler!(tsig::SigDataType, type_data_read, type_data_write),
    handler!(tsig::SigLutAtoBType, type_lut_a2b_read, type_lut_a2b_write),
    handler!(tsig::SigLutBtoAType, type_lut_b2a_read, type_lut_b2a_write),
    handler!(
        tsig::SigCrdInfoType,
        type_crd_info_read,
        type_crd_info_write
    ),
    handler!(
        tsig::SigMultiProcessElementType,
        type_mpe_read,
        type_mpe_write
    ),
    handler!(
        tsig::SigScreeningType,
        type_screening_read,
        type_screening_write
    ),
    handler!(
        tsig::SigViewingConditionsType,
        type_viewing_conditions_read,
        type_viewing_conditions_write
    ),
    handler!(tsig::SigXYZType, type_xyz_read, type_xyz_write),
    handler!(0x17A505B8, type_xyz_read, type_xyz_write), // cmsCorbisBrokenXYZtype
    handler!(0x9478ee00, type_curve_read, type_curve_write), // cmsMonacoBrokenCurveType
    handler!(
        tsig::SigProfileSequenceIdType,
        type_profile_seq_id_read,
        type_profile_seq_id_write
    ),
    handler!(tsig::SigUcrBgType, type_ucr_bg_read, type_ucr_bg_write),
    handler!(tsig::SigVcgtType, type_vcgt_read, type_vcgt_write),
    handler!(
        tsig::SigcicpType,
        type_video_signal_read,
        type_video_signal_write
    ),
    handler!(tsig::SigMHC2Type, type_mhc2_read, type_mhc2_write),
    handler!(
        tsig::SigDictType,
        type_dictionary_read,
        type_dictionary_write
    ),
    handler!(tsig::SigUInt8ArrayType, type_u8_read, type_u8_write),
    handler!(tsig::SigUInt32ArrayType, type_u32_read, type_u32_write),
    handler!(tsig::SigUInt64ArrayType, type_u64_read, type_u64_write),
];

/// `_cmsGetTagTypeHandler` — `GetHandler`: plugin list first, then defaults.
/// Owned copy (handler is `Copy`; equivalent to C's returned pointer for
/// read/write dispatch).
pub fn get_tag_type_handler(ctx: Option<&Context>, sig: u32) -> Option<TagTypeHandler> {
    if let Some(c) = ctx {
        if let Some(h) = c.tag_type_handlers.iter().find(|h| h.signature == sig) {
            return Some(*h);
        }
    }
    SUPPORTED_TAG_TYPES
        .iter()
        .find(|h| h.signature == sig)
        .copied()
}

// ---------------------------------------------------------------------------
// MLU + named-family handlers (slice B)
// ---------------------------------------------------------------------------

use crate::named::{
    alloc_named_color_list, alloc_profile_seq_desc, append_named_color, named_color_count,
    named_color_info,
};

/// `Type_MLU_Read` — multiLocalizedUnicodeType.
///
/// wchar_t is 4 bytes on this platform, so every `x * sizeof(wchar_t) /
/// sizeof(cmsUInt16Number)` becomes `x * 2` — byte counts double from the
/// utf16 domain to the utf32 pool domain.
fn type_mlu_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    mut size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut count = 0u32;
    let mut rec_len = 0u32;
    if !io::read_u32(io, &mut count) || !io::read_u32(io, &mut rec_len) {
        return None;
    }
    if rec_len != 12 {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "multiLocalizedUnicodeType of len != 12 is not supported",
        );
        return None;
    }

    let mut mlu = crate::named::mlu_alloc(None, count)?;
    mlu.used_entries = count;

    let size_of_header = 12u32.wrapping_mul(count).wrapping_add(8); // + _cmsTagBase
    let mut largest_position = 0u32;

    for i in 0..count as usize {
        let mut lang = 0u16;
        let mut country = 0u16;
        let mut len = 0u32;
        let mut offset = 0u32;
        if !io::read_u16(io, &mut lang)
            || !io::read_u16(io, &mut country)
            || !io::read_u32(io, &mut len)
            || !io::read_u32(io, &mut offset)
        {
            return None;
        }
        // Offset MUST be even — indexes utf16 chars
        if offset & 1 != 0 {
            return None;
        }
        if offset < size_of_header + 8 {
            return None;
        }
        if offset.wrapping_add(len) < len || offset.wrapping_add(len) > size_of_tag + 8 {
            return None;
        }

        let begin = offset - size_of_header - 8;
        mlu.entries[i].len = len.wrapping_mul(2); // utf16 bytes → utf32 bytes
        mlu.entries[i].str_w = begin.wrapping_mul(2);
        mlu.entries[i].language = lang;
        mlu.entries[i].country = country;

        let end = begin.wrapping_add(len);
        if end > largest_position {
            largest_position = end;
        }
    }

    // Remaining tag = utf16 block → utf32 pool
    size_of_tag = largest_position.wrapping_mul(2);
    if size_of_tag != 0 {
        if size_of_tag & 1 != 0 {
            return None;
        }
        let num_wchar = size_of_tag / 4;
        let mut wchars = Vec::new();
        if !read_wchar_array(io, num_wchar, &mut wchars) {
            return None;
        }
        // Pool bytes = size_of_tag; produced wchars fill the head, rest zeros
        let mut pool = vec![0u8; size_of_tag as usize];
        for (i, &w) in wchars.iter().enumerate() {
            pool[i * 4..i * 4 + 4].copy_from_slice(&w.to_ne_bytes());
        }
        mlu.pool = pool;
    }
    mlu.pool_used = size_of_tag;

    *n_items = 1;
    Some(TagObject::Mlu(mlu))
}

/// `Type_MLU_Write`
fn type_mlu_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let mlu = match p {
        TagObject::Mlu(m) | TagObject::Text(m) | TagObject::TextDescription(m) => m,
        // C's NULL → write empty placeholder
        _ if p.is_none_marker() => {
            return io::write_u32(io, 0) && io::write_u32(io, 12);
        }
        _ => return false,
    };

    if !io::write_u32(io, mlu.used_entries) || !io::write_u32(io, 12) {
        return false;
    }
    let header_size = 12u32.wrapping_mul(mlu.used_entries).wrapping_add(8);

    for i in 0..mlu.used_entries as usize {
        let e = &mlu.entries[i];
        // utf32-pool bytes → utf16 units
        let len = e.len.wrapping_mul(2) / 4;
        let offset = e.str_w.wrapping_mul(2) / 4 + header_size + 8;
        if !io::write_u16(io, e.language)
            || !io::write_u16(io, e.country)
            || !io::write_u32(io, len)
            || !io::write_u32(io, offset)
        {
            return false;
        }
    }

    // write pool_used/4 wchars
    let n_wchars = (mlu.pool_used / 4) as usize;
    let mut wchars = Vec::with_capacity(n_wchars);
    for i in 0..n_wchars {
        let b = &mlu.pool[i * 4..i * 4 + 4];
        wchars.push(i32::from_ne_bytes(b.try_into().unwrap()));
    }
    write_wchar_array(io, &wchars)
}

impl TagObject {
    fn is_none_marker(&self) -> bool {
        matches!(self, TagObject::Raw(v) if v.is_empty())
    }
}

/// `Type_ColorantTable_Read` — uses the named-color structure
fn type_colorant_table_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    if count > CMS_MAX_CHANNELS as u32 {
        err::signal_error(None, ERROR_RANGE, "Too many colorants");
        return None;
    }
    let mut list = alloc_named_color_list(None, count, 0, b"", b"")?;
    for _ in 0..count {
        let mut name = [0u8; 33];
        if io.read(&mut name[..32], 32, 1) != 1 {
            return None;
        }
        let mut pcs = [0u16; 3];
        if !io::read_u16_array(io, 3, Some(&mut pcs)) {
            return None;
        }
        let name_len = name.iter().position(|&b| b == 0).unwrap_or(32);
        if !append_named_color(&mut list, Some(&name[..name_len]), Some(&pcs), None) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::ColorantTable(list))
}

fn type_colorant_table_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let TagObject::ColorantTable(list) = p else {
        return false;
    };
    let n = named_color_count(list);
    if !io::write_u32(io, n) {
        return false;
    }
    for i in 0..n {
        let Some((name, _, _, pcs, _)) = named_color_info(list, i) else {
            return false;
        };
        if !io.write(32, &name[..32]) || !io::write_u16_array(io, &pcs) {
            return false;
        }
    }
    true
}

/// `Type_NamedColor_Read` — namedColor2Type
fn type_named_color_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut vendor_flag = 0u32;
    let mut count = 0u32;
    let mut n_device_coords = 0u32;
    if !io::read_u32(io, &mut vendor_flag)
        || !io::read_u32(io, &mut count)
        || !io::read_u32(io, &mut n_device_coords)
    {
        return None;
    }
    let mut prefix = [0u8; 32];
    let mut suffix = [0u8; 32];
    if io.read(&mut prefix, 32, 1) != 1 || io.read(&mut suffix, 32, 1) != 1 {
        return None;
    }
    prefix[31] = 0;
    suffix[31] = 0;

    let mut v = alloc_named_color_list(None, count, n_device_coords, &prefix, &suffix)?;
    if n_device_coords > CMS_MAX_CHANNELS as u32 {
        err::signal_error(None, ERROR_RANGE, "Too many device coordinates");
        return None;
    }
    for _ in 0..count {
        let mut root = [0u8; 33];
        let mut pcs = [0u16; 3];
        let mut colorant = [0u16; CMS_MAX_CHANNELS];
        if io.read(&mut root[..32], 32, 1) != 1 {
            return None;
        }
        if !io::read_u16_array(io, 3, Some(&mut pcs)) {
            return None;
        }
        if n_device_coords > 0 && !io::read_u16_array(io, n_device_coords, Some(&mut colorant)) {
            return None;
        }
        let nl = root.iter().position(|&b| b == 0).unwrap_or(32);
        if !append_named_color(&mut v, Some(&root[..nl]), Some(&pcs), Some(&colorant)) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::NamedColor(v))
}

fn type_named_color_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::NamedColor(list) = p else {
        return false;
    };
    let n = named_color_count(list);
    if !io::write_u32(io, 0) || !io::write_u32(io, n) || !io::write_u32(io, list.colorant_count) {
        return false;
    }
    if !io.write(32, &list.prefix[..32]) || !io.write(32, &list.suffix[..32]) {
        return false;
    }
    for i in 0..n {
        let Some((name, _, _, pcs, colorant)) = named_color_info(list, i) else {
            return false;
        };
        if !io.write(32, &name[..32])
            || !io::write_u16_array(io, &pcs)
            || !io::write_u16_array(io, &colorant[..list.colorant_count as usize])
        {
            return false;
        }
    }
    true
}

/// `ReadEmbeddedText` — dispatch on embedded type base
fn read_embedded_text(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    mlu_slot: &mut Option<Mlu>,
    size_of_tag: u32,
) -> bool {
    let base = io::read_type_base(io);
    let obj = match base {
        b if b == tsig::SigTextType => {
            let mut n = 0u32;
            type_text_read(s, io, &mut n, size_of_tag)
        }
        b if b == tsig::SigTextDescriptionType => {
            let mut n = 0u32;
            type_text_description_read(s, io, &mut n, size_of_tag)
        }
        b if b == tsig::SigMultiLocalizedUnicodeType => {
            let mut n = 0u32;
            type_mlu_read(s, io, &mut n, size_of_tag)
        }
        _ => return false,
    };
    match obj {
        Some(TagObject::Text(m))
        | Some(TagObject::TextDescription(m))
        | Some(TagObject::Mlu(m)) => {
            *mlu_slot = Some(m);
            true
        }
        _ => false,
    }
}

/// `Type_ProfileSequenceDesc_Read` — 'pseq'
fn type_profile_seq_desc_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    mut size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    if size_of_tag < 4 {
        return None;
    }
    size_of_tag -= 4;

    let mut out = alloc_profile_seq_desc(None, count)?;

    for i in 0..count as usize {
        let sec = &mut out.seq[i];
        if !io::read_u32(io, &mut sec.device_mfg) {
            return None;
        }
        if size_of_tag < 4 {
            return None;
        }
        size_of_tag -= 4;
        if !io::read_u32(io, &mut sec.device_model) {
            return None;
        }
        if size_of_tag < 4 {
            return None;
        }
        size_of_tag -= 4;
        if !io::read_u64(io, &mut sec.attributes) {
            return None;
        }
        if size_of_tag < 8 {
            return None;
        }
        size_of_tag -= 8;
        if !io::read_u32(io, &mut sec.technology) {
            return None;
        }
        if size_of_tag < 4 {
            return None;
        }
        size_of_tag -= 4;

        let mut m = sec.manufacturer.take();
        if !read_embedded_text(s, io, &mut m, size_of_tag) {
            return None;
        }
        sec.manufacturer = m;
        let mut m = sec.model.take();
        if !read_embedded_text(s, io, &mut m, size_of_tag) {
            return None;
        }
        sec.model = m;
    }
    *n_items = 1;
    Some(TagObject::ProfileSeqDesc(out))
}

/// `SaveDescription` — embeds text desc (v2) or MLU (v4) per ICCVersion
fn save_description(s: &TagTypeHandler, io: &mut IoHandler, mlu: Option<&Mlu>) -> bool {
    if s.icc_version < 0x4000000 {
        io::write_type_base(io, tsig::SigTextDescriptionType)
            && type_text_description_write(
                s,
                io,
                &TagObject::TextDescription(mlu.cloned().unwrap_or_default()),
                1,
            )
    } else {
        io::write_type_base(io, tsig::SigMultiLocalizedUnicodeType)
            && type_mlu_write(s, io, &TagObject::Mlu(mlu.cloned().unwrap_or_default()), 1)
    }
}

/// `Type_ProfileSequenceDesc_Write`
fn type_profile_seq_desc_write(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let (TagObject::ProfileSeqDesc(seq) | TagObject::ProfileSeqId(seq)) = p else {
        return false;
    };
    if !io::write_u32(io, seq.seq.len() as u32) {
        return false;
    }
    for sec in &seq.seq {
        if !io::write_u32(io, sec.device_mfg)
            || !io::write_u32(io, sec.device_model)
            || !io::write_u64(io, sec.attributes)
            || !io::write_u32(io, sec.technology)
            || !save_description(s, io, sec.manufacturer.as_ref())
            || !save_description(s, io, sec.model.as_ref())
        {
            return false;
        }
    }
    true
}

/// `Type_ProfileSequenceId_Read` — 'psid', position-table based
fn type_profile_seq_id_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8); // - sizeof(_cmsTagBase)
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    let mut out = alloc_profile_seq_desc(None, count)?;

    // Read the position table — elements are (ProfileID 16B + embedded text)
    let seq_ref = &mut out.seq;
    let ok = read_position_table(io, count, base_offset, |io, i, size_of_tag| {
        let sec = &mut seq_ref[i];
        if io.read(&mut sec.profile_id.id8, 16, 1) != 1 {
            return false;
        }
        let mut m = sec.description.take();
        let ok = read_embedded_text(s, io, &mut m, size_of_tag);
        sec.description = m;
        ok
    });
    if !ok {
        return None;
    }
    *n_items = 1;
    Some(TagObject::ProfileSeqId(out))
}

/// `Type_ProfileSequenceId_Write`
fn type_profile_seq_id_write(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let (TagObject::ProfileSeqId(seq) | TagObject::ProfileSeqDesc(seq)) = p else {
        return false;
    };
    let base_offset = io.tell().wrapping_sub(8);
    if !io::write_u32(io, seq.seq.len() as u32) {
        return false;
    }
    write_position_table(io, 0, seq.seq.len() as u32, base_offset, |io, i, _| {
        io.write(16, &seq.seq[i].profile_id.id8)
            && save_description(s, io, seq.seq[i].description.as_ref())
    })
}

/// `Type_UcrBg_Read` — UCR/BG curves + trailing ASCII desc
fn type_ucr_bg_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut signed_size = size_of_tag as i32;

    if signed_size < 4 {
        return None;
    }
    let mut count_ucr = 0u32;
    if !io::read_u32(io, &mut count_ucr) {
        return None;
    }
    signed_size -= 4;

    let mut ucr = build_tabulated_tone_curve16(None, count_ucr, None)?;
    if signed_size < (count_ucr * 2) as i32 {
        return None;
    }
    if !io::read_u16_array(io, count_ucr, Some(&mut ucr.table16)) {
        return None;
    }
    signed_size -= (count_ucr * 2) as i32;

    if signed_size < 4 {
        return None;
    }
    let mut count_bg = 0u32;
    if !io::read_u32(io, &mut count_bg) {
        return None;
    }
    signed_size -= 4;

    let mut bg = build_tabulated_tone_curve16(None, count_bg, None)?;
    if signed_size < (count_bg * 2) as i32 {
        return None;
    }
    if !io::read_u16_array(io, count_bg, Some(&mut bg.table16)) {
        return None;
    }
    signed_size -= (count_bg * 2) as i32;

    if !(0..=32000).contains(&signed_size) {
        return None;
    }

    let mut desc = crate::named::mlu_alloc(None, 1)?;
    let mut text = vec![0u8; signed_size as usize + 1];
    if io.read(&mut text, 1, signed_size as u32) != signed_size as u32 {
        return None;
    }
    let strlen = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    mlu_set_ascii(&mut desc, NO_LANG, NO_LANG, &text[..strlen]);

    *n_items = 1;
    Some(TagObject::UcrBg(UcrBg { ucr, bg, desc }))
}

fn type_ucr_bg_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::UcrBg(v) = p else { return false };
    if !io::write_u32(io, v.ucr.table16.len() as u32) || !io::write_u16_array(io, &v.ucr.table16) {
        return false;
    }
    if !io::write_u32(io, v.bg.table16.len() as u32) || !io::write_u16_array(io, &v.bg.table16) {
        return false;
    }
    let (text_size, _) = mlu_get_ascii(&v.desc, NO_LANG, NO_LANG, None);
    let (_, text) = mlu_get_ascii(&v.desc, NO_LANG, NO_LANG, Some(text_size));
    if text.len() < text_size as usize {
        return false;
    }
    io.write(text_size, &text[..text_size as usize])
}

/// `ReadCountAndString` for CrdInfo — count + raw bytes → MLU "PS"/section
fn read_count_and_string(
    io: &mut IoHandler,
    mlu: &mut Mlu,
    size_of_tag: &mut u32,
    section: &str,
) -> bool {
    if *size_of_tag < 4 {
        return false;
    }
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return false;
    }
    if count > u32::MAX - 4 {
        return false;
    }
    if *size_of_tag < count + 4 {
        return false;
    }
    let mut text = vec![0u8; count as usize + 1];
    if io.read(&mut text, 1, count) != count {
        return false;
    }
    let strlen = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    mlu_set_ascii(mlu, b"PS", section.as_bytes(), &text[..strlen]);
    *size_of_tag -= count + 4;
    true
}

/// `Type_CrdInfo_Read` — 5 PS-locale MLU entries
fn type_crd_info_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut mlu = crate::named::mlu_alloc(None, 5)?;
    let mut sz = size_of_tag;
    for section in ["nm", "#0", "#1", "#2", "#3"] {
        if !read_count_and_string(io, &mut mlu, &mut sz, section) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::CrdInfo(mlu))
}

/// `WriteCountAndString` for CrdInfo
fn write_count_and_string(io: &mut IoHandler, mlu: &Mlu, section: &str) -> bool {
    let (text_size, _) = mlu_get_ascii(mlu, b"PS", section.as_bytes(), None);
    let (_, text) = mlu_get_ascii(mlu, b"PS", section.as_bytes(), Some(text_size));
    if !io::write_u32(io, text_size) {
        return false;
    }
    if text_size == 0 {
        return false;
    }
    io.write(text_size, &text[..text_size as usize])
}

fn type_crd_info_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::CrdInfo(mlu) = p else {
        return false;
    };
    for section in ["nm", "#0", "#1", "#2", "#3"] {
        if !write_count_and_string(io, mlu, section) {
            return false;
        }
    }
    true
}

/// `Type_Screening_Read`
fn type_screening_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut flag = 0u32;
    let mut n_channels = 0u32;
    if !io::read_u32(io, &mut flag) || !io::read_u32(io, &mut n_channels) {
        return None;
    }
    if n_channels > CMS_MAX_CHANNELS as u32 - 1 {
        n_channels = CMS_MAX_CHANNELS as u32 - 1;
    }
    let mut sc = Screening {
        flag,
        channels: [ScreeningChannel::default(); CMS_MAX_CHANNELS],
        n_channels,
    };
    for i in 0..n_channels as usize {
        if !io::read_15_fixed16(io, &mut sc.channels[i].frequency)
            || !io::read_15_fixed16(io, &mut sc.channels[i].angle)
            || !io::read_u32(io, &mut sc.channels[i].spot_shape)
        {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::Screening(sc))
}

fn type_screening_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Screening(sc) = p else {
        return false;
    };
    if !io::write_u32(io, sc.flag) || !io::write_u32(io, sc.n_channels) {
        return false;
    }
    for i in 0..sc.n_channels as usize {
        if !io::write_15_fixed16(io, sc.channels[i].frequency)
            || !io::write_15_fixed16(io, sc.channels[i].angle)
            || !io::write_u32(io, sc.channels[i].spot_shape)
        {
            return false;
        }
    }
    true
}

/// `Type_ViewingConditions_Read` — 'view'
fn type_viewing_conditions_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut illuminant = CieXyz::default();
    let mut surround = CieXyz::default();
    let mut illuminant_type = 0u32;
    if !io::read_xyz(io, &mut illuminant)
        || !io::read_xyz(io, &mut surround)
        || !io::read_u32(io, &mut illuminant_type)
    {
        return None;
    }
    *n_items = 1;
    Some(TagObject::ViewingConditions(IccViewingConditions {
        illuminant_xyz: illuminant,
        surround_xyz: surround,
        illuminant_type,
    }))
}

fn type_viewing_conditions_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let TagObject::ViewingConditions(vc) = p else {
        return false;
    };
    io::write_xyz(io, &vc.illuminant_xyz)
        && io::write_xyz(io, &vc.surround_xyz)
        && io::write_u32(io, vc.illuminant_type)
}

// ---------------------------------------------------------------------------
// LUT8 / LUT16 (mft1 / mft2) — slice C
// ---------------------------------------------------------------------------

use crate::lut::{
    Pipeline, Stage, StageData, StageLoc, pipeline_alloc, stage_alloc_clut_float_granular,
    stage_alloc_clut16bit, stage_alloc_clut16bit_granular, stage_alloc_matrix,
    stage_alloc_tone_curves,
};

/// `FROM_8_TO_16`
#[inline]
fn from_8_to_16(x: u8) -> u16 {
    ((x as u16) << 8) | x as u16
}

/// `FROM_16_TO_8`
#[inline]
fn from_16_to_8(x: u16) -> u8 {
    (((x as u32) * 65281 + 8388608) >> 24) as u8
}

/// `uipow` — n*a^b with C's overflow checks (-1 on overflow)
fn uipow(n: u32, a: u32, b: u32) -> u32 {
    if a == 0 || n == 0 {
        return 0;
    }
    let mut rv = 1u32;
    for _ in 0..b {
        rv = rv.wrapping_mul(a);
        if rv > u32::MAX / a {
            return u32::MAX;
        }
    }
    let rc = rv.wrapping_mul(n);
    if rv != rc / n {
        return u32::MAX;
    }
    rc
}

/// `Read8bitTables` — nChannels 256-entry u8 tables → u16 tone curves
fn read_8bit_tables(io: &mut IoHandler, lut: &mut Pipeline, n_channels: u32) -> bool {
    if n_channels > CMS_MAX_CHANNELS as u32 || n_channels == 0 {
        return false;
    }
    let mut temp = [0u8; 256];
    let mut tables = Vec::with_capacity(n_channels as usize);
    for _ in 0..n_channels {
        let Some(t) = build_tabulated_tone_curve16(None, 256, None) else {
            return false;
        };
        tables.push(t);
    }
    for t in &mut tables {
        if io.read(&mut temp, 256, 1) != 1 {
            eprintln!("r8t: read failed pos {}", io.tell());
            return false;
        }
        for j in 0..256 {
            t.table16[j] = from_8_to_16(temp[j]);
        }
    }
    let refs: Vec<&ToneCurve> = tables.iter().collect();
    let Some(stage) = stage_alloc_tone_curves(None, n_channels, Some(&refs)) else {
        return false;
    };
    
    
    lut.insert_stage(StageLoc::AtEnd, stage)
}

/// `Write8bitTables` — C quirk: NULL Tables writes NOTHING (loop no-ops)
fn write_8bit_tables(io: &mut IoHandler, n: u32, tables: Option<&Vec<ToneCurve>>) -> bool {
    for i in 0..n as usize {
        if let Some(t) = tables {
            let c = &t[i];
            if c.n_entries == 2 && c.table16[0] == 0 && c.table16[1] == 65535 {
                for j in 0..256u32 {
                    if !io::write_u8(io, j as u8) {
                        return false;
                    }
                }
            } else if c.n_entries != 256 {
                err::signal_error(
                    None,
                    ERROR_RANGE,
                    "LUT8 needs 256 entries on prelinearization",
                );
                return false;
            } else {
                for j in 0..256 {
                    if !io::write_u8(io, from_16_to_8(c.table16[j])) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `Read16bitTables`
fn read_16bit_tables(
    io: &mut IoHandler,
    lut: &mut Pipeline,
    n_channels: u32,
    n_entries: u32,
) -> bool {
    let mut tables = Vec::with_capacity(n_channels as usize);
    for _ in 0..n_channels {
        let Some(mut t) = build_tabulated_tone_curve16(None, n_entries, None) else {
            return false;
        };
        if !io::read_u16_array(io, n_entries, Some(&mut t.table16)) {
            return false;
        }
        tables.push(t);
    }
    let refs: Vec<&ToneCurve> = tables.iter().collect();
    let Some(stage) = stage_alloc_tone_curves(None, n_channels, Some(&refs)) else {
        return false;
    };
    lut.insert_stage(StageLoc::AtEnd, stage)
}

/// `Write16bitTables`
fn write_16bit_tables(io: &mut IoHandler, tables: &Vec<ToneCurve>) -> bool {
    for c in tables {
        for j in 0..c.n_entries as usize {
            if !io::write_u16(io, c.table16[j]) {
                return false;
            }
        }
    }
    true
}

/// `mat3isIdentity` on a flat 9-double matrix
fn mat3_is_identity(m: &[f64; 9]) -> bool {
    let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    m.iter().zip(&ident).all(|(a, b)| a == b)
}

/// `Type_LUT8_Read` — mft1
fn type_lut8_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let (mut inch, mut outch, mut clutpts, mut pad) = (0u8, 0u8, 0u8, 0u8);
    if !io::read_u8(io, &mut inch) || !io::read_u8(io, &mut outch) || !io::read_u8(io, &mut clutpts)
    {
        return None;
    }
    if clutpts == 1 {
        return None;
    }
    if !io::read_u8(io, &mut pad) {
        return None;
    }
    if inch == 0 || inch > CMS_MAX_CHANNELS as u8 || outch == 0 || outch > CMS_MAX_CHANNELS as u8 {
        return None;
    }

    let mut lut = pipeline_alloc(None, inch as u32, outch as u32)?;
    let mut matrix = [0.0f64; 9];
    for m in &mut matrix {
        if !io::read_15_fixed16(io, m) {
            return None;
        }
    }
    // Only operates if not identity
    if inch == 3 && !mat3_is_identity(&matrix) {
        let Some(st) = stage_alloc_matrix(None, 3, 3, &matrix, None) else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    }
    if !read_8bit_tables(io, &mut lut, inch as u32) {
        return None;
    }
    let n_tab = uipow(outch as u32, clutpts as u32, inch as u32);
    if n_tab == u32::MAX {
        return None;
    }
    if n_tab > 0 {
        let mut temp = vec![0u8; n_tab as usize];
        if io.read(&mut temp, n_tab, 1) != 1 {
            return None;
        }
        let t: Vec<u16> = temp.iter().map(|&b| from_8_to_16(b)).collect();
        let Some(stage) =
            stage_alloc_clut16bit(None, clutpts as u32, inch as u32, outch as u32, Some(&t))
        else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtEnd, stage) {
            return None;
        }
    }
    if !read_8bit_tables(io, &mut lut, outch as u32) {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Lut8(lut))
}

/// `Type_LUT8_Write` — only [matrix,] curveset, clut, curveset allowed
fn type_lut8_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let (TagObject::Lut8(lut)
    | TagObject::Lut16(lut)
    | TagObject::LutA2B(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Mpe(lut)) = p
    else {
        return false;
    };

    // Disassemble: walk the linked list matching C's allowed order
    let mut pre: Option<&Vec<ToneCurve>> = None;
    let mut post: Option<&Vec<ToneCurve>> = None;
    let mut mat: Option<&[f64; 9]> = None;
    let mut clut: Option<&crate::lut::CLutData> = None;

    let mut mpe = lut.elements.as_deref();
    let Some(m) = mpe else {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "empty LUT8 is not supported");
        return false;
    };
    if m.stage_type() == crate::signatures::stage::SigMatrixElemType {
        if m.input_channels != 3 || m.output_channels != 3 {
            return false;
        }
        if let StageData::Matrix { double, .. } = &m.data {
            mat = Some(double.as_slice().try_into().unwrap_or(&[0.0; 9]));
        }
        mpe = m.next.as_deref();
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCurveSetElemType {
            if let StageData::ToneCurves(c) = &m.data {
                pre = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCLutElemType {
            if let StageData::CLut(c) = &m.data {
                clut = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCurveSetElemType {
            if let StageData::ToneCurves(c) = &m.data {
                post = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if mpe.is_some() {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "LUT is not suitable to be saved as LUT8",
        );
        return false;
    }

    let clut_points = match clut {
        None => 0,
        Some(c) => {
            let pts = c.n_samples[0];
            for i in 1..lut.input_channels as usize {
                if c.n_samples[i] != pts {
                    err::signal_error(
                        None,
                        ERROR_UNKNOWN_EXTENSION,
                        "different samples per dimension",
                    );
                    return false;
                }
            }
            pts
        }
    };

    if !io::write_u8(io, lut.input_channels as u8)
        || !io::write_u8(io, lut.output_channels as u8)
        || !io::write_u8(io, clut_points as u8)
        || !io::write_u8(io, 0)
    {
        return false;
    }

    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let m_out: &[f64; 9] = mat.unwrap_or(&identity);
    for &v in m_out {
        if !io::write_15_fixed16(io, v) {
            return false;
        }
    }
    if !write_8bit_tables(io, lut.input_channels, pre) {
        return false;
    }
    let n_tab = uipow(lut.output_channels, clut_points, lut.input_channels);
    if n_tab == u32::MAX {
        return false;
    }
    if n_tab > 0 {
        if let Some(c) = clut {
            for j in 0..n_tab as usize {
                if !io::write_u8(io, from_16_to_8(c.tab16.as_deref().unwrap_or(&[])[j])) {
                    return false;
                }
            }
        }
    }
    write_8bit_tables(io, lut.output_channels, post)
}

/// `Type_LUT16_Read` — mft2
fn type_lut16_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let (mut inch, mut outch, mut clutpts, mut pad) = (0u8, 0u8, 0u8, 0u8);
    if !io::read_u8(io, &mut inch)
        || !io::read_u8(io, &mut outch)
        || !io::read_u8(io, &mut clutpts)
        || !io::read_u8(io, &mut pad)
    {
        return None;
    }
    if inch == 0 || inch > CMS_MAX_CHANNELS as u8 || outch == 0 || outch > CMS_MAX_CHANNELS as u8 {
        return None;
    }

    let mut lut = pipeline_alloc(None, inch as u32, outch as u32)?;
    let mut matrix = [0.0f64; 9];
    for m in &mut matrix {
        if !io::read_15_fixed16(io, m) {
            return None;
        }
    }
    if inch == 3 && !mat3_is_identity(&matrix) {
        let Some(st) = stage_alloc_matrix(None, 3, 3, &matrix, None) else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    let (mut in_entries, mut out_entries) = (0u16, 0u16);
    if !io::read_u16(io, &mut in_entries) || !io::read_u16(io, &mut out_entries) {
        return None;
    }
    if in_entries > 0x7FFF || out_entries > 0x7FFF || clutpts == 1 {
        return None;
    }
    if !read_16bit_tables(io, &mut lut, inch as u32, in_entries as u32) {
        return None;
    }
    let n_tab = uipow(outch as u32, clutpts as u32, inch as u32);
    if n_tab == u32::MAX {
        return None;
    }
    if n_tab > 0 {
        let mut t = bytemuck::try_zeroed_vec::<u16>(n_tab as usize).ok()?;
        if !io::read_u16_array(io, n_tab, Some(&mut t)) {
            return None;
        }
        let Some(stage) =
            stage_alloc_clut16bit(None, clutpts as u32, inch as u32, outch as u32, Some(&t))
        else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtEnd, stage) {
            return None;
        }
    }
    if !read_16bit_tables(io, &mut lut, outch as u32, out_entries as u32) {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Lut16(lut))
}

/// `Type_LUT16_Write`
fn type_lut16_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let (TagObject::Lut16(lut)
    | TagObject::Lut8(lut)
    | TagObject::LutA2B(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Mpe(lut)) = p
    else {
        return false;
    };

    let mut pre: Option<&Vec<ToneCurve>> = None;
    let mut post: Option<&Vec<ToneCurve>> = None;
    let mut mat: Option<&[f64; 9]> = None;
    let mut clut: Option<&crate::lut::CLutData> = None;

    let mut mpe = lut.elements.as_deref();
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigMatrixElemType {
            if m.input_channels != 3 || m.output_channels != 3 {
                return false;
            }
            if let StageData::Matrix { double, .. } = &m.data {
                mat = Some(double.as_slice().try_into().unwrap_or(&[0.0; 9]));
            }
            mpe = m.next.as_deref();
        }
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCurveSetElemType {
            if let StageData::ToneCurves(c) = &m.data {
                pre = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCLutElemType {
            if let StageData::CLut(c) = &m.data {
                clut = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if let Some(m) = mpe {
        if m.stage_type() == crate::signatures::stage::SigCurveSetElemType {
            if let StageData::ToneCurves(c) = &m.data {
                post = Some(c);
            }
            mpe = m.next.as_deref();
        }
    }
    if mpe.is_some() {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "LUT is not suitable to be saved as LUT16",
        );
        return false;
    }

    let (in_ch, out_ch) = (lut.input_channels, lut.output_channels);
    let clut_points = match clut {
        None => 0,
        Some(c) => {
            let pts = c.n_samples[0];
            for i in 1..in_ch as usize {
                if c.n_samples[i] != pts {
                    err::signal_error(
                        None,
                        ERROR_UNKNOWN_EXTENSION,
                        "different samples per dimension",
                    );
                    return false;
                }
            }
            pts
        }
    };

    if !io::write_u8(io, in_ch as u8)
        || !io::write_u8(io, out_ch as u8)
        || !io::write_u8(io, clut_points as u8)
        || !io::write_u8(io, 0)
    {
        return false;
    }
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    for &v in mat.unwrap_or(&identity) {
        if !io::write_15_fixed16(io, v) {
            return false;
        }
    }
    let in_entries = pre.map(|c| c[0].n_entries).unwrap_or(2);
    let out_entries = post.map(|c| c[0].n_entries).unwrap_or(2);
    if !io::write_u16(io, in_entries as u16) || !io::write_u16(io, out_entries as u16) {
        return false;
    }
    match pre {
        Some(t) => {
            if !write_16bit_tables(io, t) {
                return false;
            }
        }
        None => {
            for _ in 0..in_ch {
                if !io::write_u16(io, 0) || !io::write_u16(io, 0xffff) {
                    return false;
                }
            }
        }
    }
    let n_tab = uipow(out_ch, clut_points, in_ch);
    if n_tab == u32::MAX {
        return false;
    }
    if n_tab > 0 {
        if let Some(c) = clut {
            if !io::write_u16_array(io, &c.tab16.as_deref().unwrap_or(&[])[..n_tab as usize]) {
                return false;
            }
        }
    }
    match post {
        Some(t) => write_16bit_tables(io, t),
        None => {
            for _ in 0..out_ch {
                if !io::write_u16(io, 0) || !io::write_u16(io, 0xffff) {
                    return false;
                }
            }
            true
        }
    }
}

/// `ReadMatrix` — seek + 9 doubles + 3 offsets
fn read_matrix(io: &mut IoHandler, offset: u32) -> Option<Stage> {
    if !io.seek(offset) {
        return None;
    }
    let mut dmat = [0.0f64; 9];
    let mut doff = [0.0f64; 3];
    for v in &mut dmat {
        if !io::read_15_fixed16(io, v) {
            return None;
        }
    }
    for v in &mut doff {
        if !io::read_15_fixed16(io, v) {
            return None;
        }
    }
    stage_alloc_matrix(None, 3, 3, &dmat, Some(&doff))
}

/// `ReadCLUT` — v4 CLUT: seek + 16B grid + precision + 3 pads
fn read_clut(io: &mut IoHandler, offset: u32, in_ch: u32, out_ch: u32) -> Option<Stage> {
    if !io.seek(offset) {
        return None;
    }
    let mut grid8 = [0u8; CMS_MAX_CHANNELS];
    if io.read(&mut grid8, CMS_MAX_CHANNELS as u32, 1) != 1 {
        return None;
    }
    let mut grid = [0u32; CMS_MAX_CHANNELS];
    for i in 0..CMS_MAX_CHANNELS {
        if grid8[i] == 1 {
            return None;
        }
        grid[i] = grid8[i] as u32;
    }
    let mut precision = 0u8;
    if !io::read_u8(io, &mut precision) {
        return None;
    }
    let mut pad = 0u8;
    for _ in 0..3 {
        if !io::read_u8(io, &mut pad) {
            return None;
        }
    }
    let mut clut = stage_alloc_clut16bit_granular(None, &grid, in_ch, out_ch, None)?;
    if let StageData::CLut(d) = &mut clut.data {
        match precision {
            1 => {
                for i in 0..d.n_entries as usize {
                    let mut v = 0u8;
                    if io.read(std::slice::from_mut(&mut v), 1, 1) != 1 {
                        return None;
                    }
                    if let Some(t) = d.tab16.as_mut().map(std::sync::Arc::make_mut) {
                        t[i] = from_8_to_16(v);
                    }
                }
            }
            2 => {
                if let Some(t) = d.tab16.as_mut().map(std::sync::Arc::make_mut) {
                    if !io::read_u16_array(io, d.n_entries, Some(t)) {
                        return None;
                    }
                }
            }
            _ => {
                err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown precision");
                return None;
            }
        }
        // `Arc::make_mut` swapped the table — re-point the prebuilt params.
        if let Some(p) = &mut d.params16 {
            p.table = crate::intrp::InterpTable::U16Rc(
                d.tab16.clone().unwrap_or_default(),
            );
        }
    }
    Some(clut)
}

/// `ReadEmbeddedCurve`
fn read_embedded_curve(s: &TagTypeHandler, io: &mut IoHandler) -> Option<ToneCurve> {
    let base = io::read_type_base(io);
    let mut n = 0u32;
    match base {
        b if b == tsig::SigCurveType => match type_curve_read(s, io, &mut n, 0) {
            Some(TagObject::Curve(c)) => Some(c),
            _ => None,
        },
        b if b == tsig::SigParametricCurveType => {
            match type_parametric_curve_read(s, io, &mut n, 0) {
                Some(TagObject::ParametricCurve(c)) => Some(c),
                _ => None,
            }
        }
        _ => {
            err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown curve type");
            None
        }
    }
}

/// `ReadSetOfCurves` — v4: type-base'd curves at an offset
fn read_set_of_curves(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    offset: u32,
    n_curves: u32,
) -> Option<Stage> {
    if n_curves > CMS_MAX_CHANNELS as u32 || !io.seek(offset) {
        return None;
    }
    let mut curves = Vec::with_capacity(n_curves as usize);
    for _ in 0..n_curves {
        let c = read_embedded_curve(s, io)?;
        curves.push(c);
        if !io::read_alignment(io) {
            return None;
        }
    }
    let refs: Vec<&ToneCurve> = curves.iter().collect();
    stage_alloc_tone_curves(None, n_curves, Some(&refs))
}

/// `Type_LUTA2B_Read` — 'mAB '
fn type_lut_a2b_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let (mut inch, mut outch, mut pad) = (0u8, 0u8, 0u16);
    if !io::read_u8(io, &mut inch) || !io::read_u8(io, &mut outch) || !io::read_u16(io, &mut pad) {
        return None;
    }
    let mut off_b = 0u32;
    let mut off_mat = 0u32;
    let mut off_m = 0u32;
    let mut off_c = 0u32;
    let mut off_a = 0u32;
    if !io::read_u32(io, &mut off_b)
        || !io::read_u32(io, &mut off_mat)
        || !io::read_u32(io, &mut off_m)
        || !io::read_u32(io, &mut off_c)
        || !io::read_u32(io, &mut off_a)
    {
        return None;
    }
    if inch == 0 || inch >= CMS_MAX_CHANNELS as u8 || outch == 0 || outch >= CMS_MAX_CHANNELS as u8
    {
        return None;
    }
    let mut lut = pipeline_alloc(None, inch as u32, outch as u32)?;
    // A, CLUT, M, Matrix, B order
    for (off, nc) in [
        (off_a, inch as u32),
        (0, 0), // clut handled below
        (off_m, outch as u32),
        (0, 0), // matrix below
        (off_b, outch as u32),
    ] {
        let _ = (off, nc);
    }
    if off_a != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_a), inch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_c != 0 {
        let st = read_clut(
            io,
            base_offset.wrapping_add(off_c),
            inch as u32,
            outch as u32,
        )?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_m != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_m), outch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_mat != 0 {
        let st = read_matrix(io, base_offset.wrapping_add(off_mat))?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_b != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_b), outch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::LutA2B(lut))
}

/// `Type_LUTB2A_Read` — 'mBA '
fn type_lut_b2a_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let (mut inch, mut outch, mut pad) = (0u8, 0u8, 0u16);
    if !io::read_u8(io, &mut inch) || !io::read_u8(io, &mut outch) {
        return None;
    }
    if inch == 0 || inch >= CMS_MAX_CHANNELS as u8 || outch == 0 || outch >= CMS_MAX_CHANNELS as u8
    {
        return None;
    }
    if !io::read_u16(io, &mut pad) {
        return None;
    }
    let mut off_b = 0u32;
    let mut off_mat = 0u32;
    let mut off_m = 0u32;
    let mut off_c = 0u32;
    let mut off_a = 0u32;
    if !io::read_u32(io, &mut off_b)
        || !io::read_u32(io, &mut off_mat)
        || !io::read_u32(io, &mut off_m)
        || !io::read_u32(io, &mut off_c)
        || !io::read_u32(io, &mut off_a)
    {
        return None;
    }
    let mut lut = pipeline_alloc(None, inch as u32, outch as u32)?;
    // B, Matrix, M, CLUT, A order
    if off_b != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_b), inch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_mat != 0 {
        let st = read_matrix(io, base_offset.wrapping_add(off_mat))?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_m != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_m), inch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_c != 0 {
        let st = read_clut(
            io,
            base_offset.wrapping_add(off_c),
            inch as u32,
            outch as u32,
        )?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    if off_a != 0 {
        let st = read_set_of_curves(s, io, base_offset.wrapping_add(off_a), outch as u32)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::LutB2A(lut))
}

/// `WriteMatrix` — n doubles + out offsets (zeros if none)
fn write_matrix(io: &mut IoHandler, mpe: &Stage) -> bool {
    let StageData::Matrix { double, offset } = &mpe.data else {
        return false;
    };
    let n = (mpe.input_channels * mpe.output_channels) as usize;
    for v in &double[..n] {
        if !io::write_15_fixed16(io, *v) {
            return false;
        }
    }
    match offset {
        Some(off) => {
            for &v in &off[..mpe.output_channels as usize] {
                if !io::write_15_fixed16(io, v) {
                    return false;
                }
            }
        }
        None => {
            for _ in 0..mpe.output_channels {
                if !io::write_15_fixed16(io, 0.0) {
                    return false;
                }
            }
        }
    }
    true
}

/// `WriteSetOfCurves` — each curve type-base'd; tabulated → 'curv', else 'para'
fn write_set_of_curves(s: &TagTypeHandler, io: &mut IoHandler, typ: u32, mpe: &Stage) -> bool {
    let StageData::ToneCurves(curves) = &mpe.data else {
        return false;
    };
    for c in curves.iter().take(mpe.output_channels as usize) {
        let current = if c.segments.is_empty()
            || (c.segments.len() == 3 && c.segments[1].r#type == 0)
            || (!c.segments.is_empty() && c.segments[0].r#type < 0)
        {
            tsig::SigCurveType
        } else {
            typ
        };
        let ok = if !io::write_type_base(io, current) {
            return false;
        } else if current == tsig::SigCurveType {
            type_curve_write(s, io, &TagObject::Curve(c.clone()), 1)
        } else if current == tsig::SigParametricCurveType {
            type_parametric_curve_write(s, io, &TagObject::ParametricCurve(c.clone()), 1)
        } else {
            err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown curve type");
            false
        };
        if !ok || !io::write_alignment(io) {
            return false;
        }
    }
    true
}

/// `WriteCLUT` — grid + precision + data
fn write_clut(io: &mut IoHandler, precision: u8, mpe: &Stage) -> bool {
    let StageData::CLut(clut) = &mpe.data else {
        return false;
    };
    if clut.has_float_values {
        err::signal_error(
            None,
            ERROR_NOT_SUITABLE,
            "Cannot save floating point data, CLUT are 8 or 16 bit only",
        );
        return false;
    }
    let mut grid = [0u8; CMS_MAX_CHANNELS];
    for i in 0..clut.n_inputs as usize {
        grid[i] = clut.n_samples[i] as u8;
    }
    if !io.write(CMS_MAX_CHANNELS as u32, &grid)
        || !io::write_u8(io, precision)
        || !io::write_u8(io, 0)
        || !io::write_u8(io, 0)
        || !io::write_u8(io, 0)
    {
        return false;
    }
    let tab = clut.tab16.as_deref().unwrap_or(&[]);
    match precision {
        1 => {
            for &v in &tab[..clut.n_entries as usize] {
                if !io::write_u8(io, from_16_to_8(v)) {
                    return false;
                }
            }
        }
        2 => {
            if !io::write_u16_array(io, &tab[..clut.n_entries as usize]) {
                return false;
            }
        }
        _ => {
            err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown precision");
            return false;
        }
    }
    io::write_alignment(io)
}

/// `cmsPipelineCheckAndRetreiveStages` — match the whole stage list to a signature sequence
fn pipeline_stage_match<'a>(lut: &'a Pipeline, sigs: &[u32]) -> Option<Vec<&'a Stage>> {
    let mut out = Vec::with_capacity(sigs.len());
    let mut mpe = lut.elements.as_deref();
    for &s in sigs {
        let m = mpe?;
        if m.stage_type() != s {
            return None;
        }
        out.push(m);
        mpe = m.next.as_deref();
    }
    // C requires the stage count to match exactly — fail if any remain
    if mpe.is_some() {
        return None;
    }
    Some(out)
}

use crate::signatures::stage as ssig;

/// Shared A2B/B2A writer — same section layout; `is_b2a` picks the stage patterns.
fn type_lut_v4_write(s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, is_b2a: bool) -> bool {
    let (TagObject::LutA2B(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Lut8(lut)
    | TagObject::Lut16(lut)
    | TagObject::Mpe(lut)) = p
    else {
        return false;
    };
    let base_offset = io.tell().wrapping_sub(8);

    // Resolve A/B/M/Matrix/CLUT stages via the allowed shape checks
    let (mut a, mut b, mut m, mut matrix, mut clut) = (None, None, None, None, None);
    if lut.elements.is_some() {
        let ok = if is_b2a {
            if let Some(v) = pipeline_stage_match(lut, &[ssig::SigCurveSetElemType]) {
                b = v.first().copied();
                true
            } else if let Some(v) = pipeline_stage_match(
                lut,
                &[
                    ssig::SigCurveSetElemType,
                    ssig::SigMatrixElemType,
                    ssig::SigCurveSetElemType,
                ],
            ) {
                b = v.first().copied();
                matrix = v.get(1).copied();
                m = v.get(2).copied();
                true
            } else if let Some(v) = pipeline_stage_match(
                lut,
                &[
                    ssig::SigCurveSetElemType,
                    ssig::SigCLutElemType,
                    ssig::SigCurveSetElemType,
                ],
            ) {
                b = v.first().copied();
                clut = v.get(1).copied();
                a = v.get(2).copied();
                true
            } else if let Some(v) = pipeline_stage_match(
                lut,
                &[
                    ssig::SigCurveSetElemType,
                    ssig::SigMatrixElemType,
                    ssig::SigCurveSetElemType,
                    ssig::SigCLutElemType,
                    ssig::SigCurveSetElemType,
                ],
            ) {
                b = v.first().copied();
                matrix = v.get(1).copied();
                m = v.get(2).copied();
                clut = v.get(3).copied();
                a = v.get(4).copied();
                true
            } else {
                false
            }
        } else if let Some(v) = pipeline_stage_match(lut, &[ssig::SigCurveSetElemType]) {
            b = v.first().copied();
            true
        } else if let Some(v) = pipeline_stage_match(
            lut,
            &[
                ssig::SigCurveSetElemType,
                ssig::SigMatrixElemType,
                ssig::SigCurveSetElemType,
            ],
        ) {
            m = v.first().copied();
            matrix = v.get(1).copied();
            b = v.get(2).copied();
            true
        } else if let Some(v) = pipeline_stage_match(
            lut,
            &[
                ssig::SigCurveSetElemType,
                ssig::SigCLutElemType,
                ssig::SigCurveSetElemType,
            ],
        ) {
            a = v.first().copied();
            clut = v.get(1).copied();
            b = v.get(2).copied();
            true
        } else if let Some(v) = pipeline_stage_match(
            lut,
            &[
                ssig::SigCurveSetElemType,
                ssig::SigCLutElemType,
                ssig::SigCurveSetElemType,
                ssig::SigMatrixElemType,
                ssig::SigCurveSetElemType,
            ],
        ) {
            a = v.first().copied();
            clut = v.get(1).copied();
            m = v.get(2).copied();
            matrix = v.get(3).copied();
            b = v.get(4).copied();
            true
        } else {
            false
        };
        if !ok {
            err::signal_error(
                None,
                ERROR_NOT_SUITABLE,
                "LUT is not suitable to be saved as v4",
            );
            return false;
        }
    }

    let (inch, outch) = (lut.input_channels, lut.output_channels);
    if !io::write_u8(io, inch as u8) || !io::write_u8(io, outch as u8) || !io::write_u16(io, 0) {
        return false;
    }
    let dir_pos = io.tell();
    for _ in 0..5 {
        if !io::write_u32(io, 0) {
            return false;
        }
    }

    let (mut off_a, mut off_b, mut off_m, mut off_mat, mut off_c) = (0u32, 0u32, 0u32, 0u32, 0u32);
    if let Some(st) = a {
        off_a = io.tell().wrapping_sub(base_offset);
        if !write_set_of_curves(s, io, tsig::SigParametricCurveType, st) {
            return false;
        }
    }
    if let Some(st) = clut {
        off_c = io.tell().wrapping_sub(base_offset);
        if !write_clut(io, if lut.save_as_8bits { 1 } else { 2 }, st) {
            return false;
        }
    }
    if let Some(st) = m {
        off_m = io.tell().wrapping_sub(base_offset);
        if !write_set_of_curves(s, io, tsig::SigParametricCurveType, st) {
            return false;
        }
    }
    if let Some(st) = matrix {
        off_mat = io.tell().wrapping_sub(base_offset);
        if !write_matrix(io, st) {
            return false;
        }
    }
    if let Some(st) = b {
        off_b = io.tell().wrapping_sub(base_offset);
        if !write_set_of_curves(s, io, tsig::SigParametricCurveType, st) {
            return false;
        }
    }

    let cur_pos = io.tell();
    if !io.seek(dir_pos)
        || !io::write_u32(io, off_b)
        || !io::write_u32(io, off_mat)
        || !io::write_u32(io, off_m)
        || !io::write_u32(io, off_c)
        || !io::write_u32(io, off_a)
    {
        return false;
    }
    io.seek(cur_pos)
}

fn type_lut_a2b_write(s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    type_lut_v4_write(s, io, p, false)
}
fn type_lut_b2a_write(s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    type_lut_v4_write(s, io, p, true)
}

// ---------------------------------------------------------------------------
// MPE (multiProcessElementsType) — 'mpet' + element handlers
// ---------------------------------------------------------------------------

const SEGMENTED_CURVE_SIG: u32 = 0x63757266; // 'segf'
const FORMULA_CURVE_SEG: u32 = 0x70617266; // 'parf'
const SAMPLED_CURVE_SEG: u32 = 0x73616d66; // 'samf'
const MINUS_INF: f32 = f32::NEG_INFINITY;
const PLUS_INF: f32 = f32::INFINITY;
const MAX_INPUT_DIMENSIONS: usize = 15;

/// `ReadSegmentedCurve`
fn read_segmented_curve(_s: &TagTypeHandler, io: &mut IoHandler) -> Option<ToneCurve> {
    let mut elem_sig = 0u32;
    if !io::read_u32(io, &mut elem_sig) {
        return None;
    }
    if elem_sig != SEGMENTED_CURVE_SIG {
        return None;
    }
    let mut res = 0u32;
    let mut n_segments = 0u16;
    let mut res16 = 0u16;
    if !io::read_u32(io, &mut res)
        || !io::read_u16(io, &mut n_segments)
        || !io::read_u16(io, &mut res16)
    {
        return None;
    }
    if n_segments < 1 {
        return None;
    }
    let n = n_segments as usize;
    let mut segments = vec![CurveSegment::default(); n];
    let mut prev_break = MINUS_INF;
    for i in 0..n - 1 {
        segments[i].x0 = prev_break;
        if !io::read_float32(io, &mut segments[i].x1) {
            return None;
        }
        prev_break = segments[i].x1;
    }
    segments[n - 1].x0 = prev_break;
    segments[n - 1].x1 = PLUS_INF;

    for i in 0..n {
        let mut sig = 0u32;
        if !io::read_u32(io, &mut sig) || !io::read_u32(io, &mut res) {
            return None;
        }
        match sig {
            FORMULA_CURVE_SEG => {
                let mut ty = 0u16;
                if !io::read_u16(io, &mut ty) || !io::read_u16(io, &mut res16) {
                    return None;
                }
                segments[i].r#type = ty as i32 + 6;
                if ty > 2 {
                    return None;
                }
                const PARAMS_BY_TYPE: [usize; 3] = [4, 5, 5];
                for j in 0..PARAMS_BY_TYPE[ty as usize] {
                    let mut f = 0.0f32;
                    if !io::read_float32(io, &mut f) {
                        return None;
                    }
                    segments[i].params[j] = f as f64;
                }
            }
            SAMPLED_CURVE_SEG => {
                let mut count = 0u32;
                if !io::read_u32(io, &mut count) {
                    return None;
                }
                // First point is implicit in the last stage — extra slot
                count += 1;
                segments[i].n_grid_points = count;
                let mut pts = vec![0.0f32; count as usize];
                pts[0] = 0.0;
                for j in 1..count as usize {
                    if !io::read_float32(io, &mut pts[j]) {
                        return None;
                    }
                }
                segments[i].sampled_points = Some(pts);
            }
            _ => {
                err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown curve element type");
                return None;
            }
        }
    }

    let curve = build_segmented_tone_curve(None, &segments)?;

    // Explore for missing implicit points
    let mut curve = curve;
    for i in 0..n {
        if curve.segments[i].r#type == 0 {
            {
                let v = curve.eval_float(curve.segments[i].x0);
                if let Some(sp) = &mut curve.segments[i].sampled_points {
                    sp[0] = v;
                }
            }
        }
    }
    Some(curve)
}

/// `Type_MPEcurve_Read` — position table of segmented curves
fn type_mpe_curve_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let (mut in_ch, mut out_ch) = (0u16, 0u16);
    if !io::read_u16(io, &mut in_ch) || !io::read_u16(io, &mut out_ch) {
        return None;
    }
    if in_ch != out_ch {
        return None;
    }
    let n = in_ch as usize;
    let mut tables: Vec<Option<ToneCurve>> = vec![None; n];
    let ok = read_position_table(io, in_ch as u32, base_offset, |io, i, _| {
        let c = read_segmented_curve(s, io);
        tables[i] = c;
        tables[i].is_some()
    });
    if !ok {
        return None;
    }
    let owned: Vec<ToneCurve> = tables.into_iter().flatten().collect();
    let refs: Vec<&ToneCurve> = owned.iter().collect();
    let stage = stage_alloc_tone_curves(None, in_ch as u32, Some(&refs))?;
    *n_items = 1;
    Some(TagObject::MpeElem(stage))
}

/// `WriteSegmentedCurve` — no validity checks (C semantics)
fn write_segmented_curve(io: &mut IoHandler, g: &ToneCurve) -> bool {
    let n = g.segments.len();
    if !io::write_u32(io, SEGMENTED_CURVE_SIG)
        || !io::write_u32(io, 0)
        || !io::write_u16(io, n as u16)
        || !io::write_u16(io, 0)
    {
        return false;
    }
    for i in 0..n - 1 {
        if !io::write_float32(io, g.segments[i].x1) {
            return false;
        }
    }
    for i in 0..n {
        let seg = &g.segments[i];
        if seg.r#type == 0 {
            if !io::write_u32(io, SAMPLED_CURVE_SEG)
                || !io::write_u32(io, 0)
                || !io::write_u32(io, seg.n_grid_points - 1)
            {
                return false;
            }
            for j in 1..seg.n_grid_points as usize {
                if !io::write_float32(io, seg.sampled_points.as_deref().unwrap_or(&[])[j]) {
                    return false;
                }
            }
        } else {
            let ty = seg.r#type - 6;
            if !(0..=2).contains(&ty) {
                return false;
            }
            if !io::write_u32(io, FORMULA_CURVE_SEG)
                || !io::write_u32(io, 0)
                || !io::write_u16(io, ty as u16)
                || !io::write_u16(io, 0)
            {
                return false;
            }
            const PARAMS_BY_TYPE: [usize; 3] = [4, 5, 5];
            for j in 0..PARAMS_BY_TYPE[ty as usize] {
                if !io::write_float32(io, seg.params[j] as f32) {
                    return false;
                }
            }
        }
    }
    true
}

/// `Type_MPEcurve_Write`
fn type_mpe_curve_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::MpeElem(mpe) = p else {
        return false;
    };
    let StageData::ToneCurves(curves) = &mpe.data else {
        return false;
    };
    let base_offset = io.tell().wrapping_sub(8);
    if !io::write_u16(io, mpe.input_channels as u16)
        || !io::write_u16(io, mpe.input_channels as u16)
    {
        return false;
    }
    write_position_table(io, 0, mpe.input_channels, base_offset, |io, i, _| {
        write_segmented_curve(io, &curves[i])
    })
}

/// `Type_MPEmatrix_Read` — f32 matrix + offsets → f64 stage
fn type_mpe_matrix_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let (mut in_ch, mut out_ch) = (0u16, 0u16);
    if !io::read_u16(io, &mut in_ch) || !io::read_u16(io, &mut out_ch) {
        return None;
    }
    if in_ch >= CMS_MAX_CHANNELS as u16 || out_ch >= CMS_MAX_CHANNELS as u16 {
        return None;
    }
    let n_elems = in_ch as usize * out_ch as usize;
    let mut matrix = vec![0.0f64; n_elems];
    let mut offsets = vec![0.0f64; out_ch as usize];
    for v in &mut matrix {
        let mut f = 0.0f32;
        if !io::read_float32(io, &mut f) {
            return None;
        }
        *v = f as f64;
    }
    for v in &mut offsets {
        let mut f = 0.0f32;
        if !io::read_float32(io, &mut f) {
            return None;
        }
        *v = f as f64;
    }
    let mpe = stage_alloc_matrix(None, out_ch as u32, in_ch as u32, &matrix, Some(&offsets))?;
    *n_items = 1;
    Some(TagObject::MpeElem(mpe))
}

/// `Type_MPEmatrix_Write`
fn type_mpe_matrix_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::MpeElem(mpe) = p else {
        return false;
    };
    let StageData::Matrix { double, offset } = &mpe.data else {
        return false;
    };
    if !io::write_u16(io, mpe.input_channels as u16)
        || !io::write_u16(io, mpe.output_channels as u16)
    {
        return false;
    }
    for &v in double
        .iter()
        .take((mpe.input_channels * mpe.output_channels) as usize)
    {
        if !io::write_float32(io, v as f32) {
            return false;
        }
    }
    for i in 0..mpe.output_channels as usize {
        let v = offset.as_ref().map(|o| o[i]).unwrap_or(0.0);
        if !io::write_float32(io, v as f32) {
            return false;
        }
    }
    true
}

/// `Type_MPEclut_Read` — float32 CLUT, 16-byte dims
fn type_mpe_clut_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let (mut in_ch, mut out_ch) = (0u16, 0u16);
    if !io::read_u16(io, &mut in_ch) || !io::read_u16(io, &mut out_ch) {
        return None;
    }
    if in_ch == 0
        || in_ch >= CMS_MAX_CHANNELS as u16
        || out_ch == 0
        || out_ch >= CMS_MAX_CHANNELS as u16
    {
        return None;
    }
    let mut dims8 = [0u8; 16];
    if io.read(&mut dims8, 1, 16) != 16 {
        return None;
    }
    let n_max = (in_ch as usize).min(MAX_INPUT_DIMENSIONS);
    let mut grid = [0u32; MAX_INPUT_DIMENSIONS];
    for i in 0..n_max {
        if dims8[i] == 1 {
            return None;
        }
        grid[i] = dims8[i] as u32;
    }
    let mut mpe = stage_alloc_clut_float_granular(None, &grid, in_ch as u32, out_ch as u32, None)?;
    if let StageData::CLut(clut) = &mut mpe.data {
        if let Some(t) = clut.tab_float.as_mut().map(std::sync::Arc::make_mut) {
            for v in t.iter_mut() {
                if !io::read_float32(io, v) {
                    return None;
                }
            }
        }
        if let Some(p) = &mut clut.params_f {
            p.table = crate::intrp::InterpTable::F32Rc(
                clut.tab_float.clone().unwrap_or_default(),
            );
        }
    }
    *n_items = 1;
    Some(TagObject::MpeElem(mpe))
}

/// `Type_MPEclut_Write`
fn type_mpe_clut_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::MpeElem(mpe) = p else {
        return false;
    };
    let StageData::CLut(clut) = &mpe.data else {
        return false;
    };
    if mpe.input_channels > MAX_INPUT_DIMENSIONS as u32 || !clut.has_float_values {
        return false;
    }
    if !io::write_u16(io, mpe.input_channels as u16)
        || !io::write_u16(io, mpe.output_channels as u16)
    {
        return false;
    }
    let mut dims = [0u8; 16];
    for i in 0..mpe.input_channels as usize {
        dims[i] = clut.n_samples[i] as u8;
    }
    if !io.write(16, &dims) {
        return false;
    }
    for &v in clut.tab_float.as_deref().unwrap_or(&[]) {
        if !io::write_float32(io, v) {
            return false;
        }
    }
    true
}

/// `_cmsMPEtypehandler_struct` — element handlers can be NULL (ignored elems)
#[derive(Clone, Copy)]
pub struct MpeElemHandler {
    pub signature: u32,
    pub read: Option<fn(&TagTypeHandler, &mut IoHandler, &mut u32, u32) -> Option<TagObject>>,
    pub write: Option<fn(&TagTypeHandler, &mut IoHandler, &TagObject, u32) -> bool>,
    /// nonzero → a C-registered `cmsPluginMultiProcessElement` handler; the
    /// value is the FFI's C-ABI `cmsTagTypeHandler` box (`self` arg).
    pub foreign: usize,
}

/// `SupportedMPEtypes` — MPE element handler table (bACS/eACS = NULL, ignored)
pub static SUPPORTED_MPE_TYPES: &[MpeElemHandler] = &[
    MpeElemHandler {
        signature: 0x62414373,
        read: None,
        write: None,
        foreign: 0,
    }, // 'bACS'
    MpeElemHandler {
        signature: 0x65414373,
        read: None,
        write: None,
        foreign: 0,
    }, // 'eACS'
    MpeElemHandler {
        signature: crate::signatures::stage::SigCurveSetElemType,
        read: Some(type_mpe_curve_read),
        write: Some(type_mpe_curve_write),
        foreign: 0,
    },
    MpeElemHandler {
        signature: crate::signatures::stage::SigMatrixElemType,
        read: Some(type_mpe_matrix_read),
        write: Some(type_mpe_matrix_write),
        foreign: 0,
    },
    MpeElemHandler {
        signature: crate::signatures::stage::SigCLutElemType,
        read: Some(type_mpe_clut_read),
        write: Some(type_mpe_clut_write),
        foreign: 0,
    },
];

/// A plugin-registered MPE element handler (`cbox` = FFI `cmsTagTypeHandler`).
pub fn foreign_mpe_elem_handler(signature: u32, cbox: usize) -> MpeElemHandler {
    MpeElemHandler {
        signature,
        read: Some(foreign_read),
        write: Some(foreign_write),
        foreign: cbox,
    }
}

fn mpe_handler(sig: u32, ctx_token: usize) -> Option<MpeElemHandler> {
    // plugin handlers precede built-ins — the FFI resolves the ctx token
    if ctx_token != 0 {
        if let Some(lk) = *crate::context::MPE_LOOKUP.read().unwrap() {
            if let Some(h) = lk(ctx_token, sig) {
                return Some(h);
            }
        }
    }
    SUPPORTED_MPE_TYPES
        .iter()
        .find(|h| h.signature == sig)
        .copied()
}

/// `ReadMPEElem` — element sig + reserved, dispatch
fn read_mpe_elem(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    lut: &mut Pipeline,
    size_of_tag: u32,
) -> bool {
    let mut elem_sig = 0u32;
    let mut res = 0u32;
    if !io::read_u32(io, &mut elem_sig) || !io::read_u32(io, &mut res) {
        return false;
    }
    // `self->ContextID` — stamped on the handler copy by the caller
    let Some(h) = mpe_handler(elem_sig, s.ctx) else {
        err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Unknown MPE type found");
        return false;
    };
    // Foreign (C-registered) element — `ReadPtr` returns `cmsStage*` = our
    // boxed `Stage` produced by `_cmsStageAllocPlaceholder`.
    if h.foreign != 0 {
        let Some(hooks) = *crate::context::TAGTYPE_HOOKS.read().unwrap() else {
            return false;
        };
        let Some(box_stage) = *crate::context::STAGE_BOX.read().unwrap() else {
            return false;
        };
        let mut n = 0u32;
        let ptr = (hooks.read)(
            h.foreign,
            s.icc_version,
            io as *mut IoHandler as usize,
            &mut n as *mut u32 as usize,
            size_of_tag as usize,
        );
        let Some(stage) = box_stage(ptr) else {
            return false;
        };
        return lut.insert_stage(StageLoc::AtEnd, *stage);
    }
    // NULL read → ignored element (bACS/eACS)
    if let Some(read) = h.read {
        let mut n = 0u32;
        match read(s, io, &mut n, size_of_tag) {
            Some(TagObject::MpeElem(stage)) => return lut.insert_stage(StageLoc::AtEnd, stage),
            _ => return false,
        }
    }
    true
}

/// `Type_MPE_Read` — 'mpet' container
fn type_mpe_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let (mut in_ch, mut out_ch) = (0u16, 0u16);
    if !io::read_u16(io, &mut in_ch) || !io::read_u16(io, &mut out_ch) {
        return None;
    }
    if in_ch == 0
        || in_ch >= CMS_MAX_CHANNELS as u16
        || out_ch == 0
        || out_ch >= CMS_MAX_CHANNELS as u16
    {
        return None;
    }
    let mut lut = pipeline_alloc(None, in_ch as u32, out_ch as u32)?;
    let mut elem_count = 0u32;
    if !io::read_u32(io, &mut elem_count) {
        return None;
    }
    if elem_count > 50 {
        return None;
    }
    let lut_ref = &mut lut;
    let ok = read_position_table(io, elem_count, base_offset, |io, _, size_of_tag| {
        read_mpe_elem(s, io, lut_ref, size_of_tag)
    });
    if !ok {
        return None;
    }
    if in_ch as u32 != lut.input_channels || out_ch as u32 != lut.output_channels {
        return None;
    }
    *n_items = 1;
    Some(TagObject::Mpe(lut))
}

/// `Type_MPE_Write` — hand-rolled directory back-patch (no position table)
fn type_mpe_write(s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let (TagObject::Mpe(lut)
    | TagObject::LutA2B(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Lut8(lut)
    | TagObject::Lut16(lut)) = p
    else {
        return false;
    };
    let base_offset = io.tell().wrapping_sub(8);
    let (in_ch, out_ch) = (lut.input_channels, lut.output_channels);

    // Collect stages
    let mut elems: Vec<&Stage> = Vec::new();
    let mut e = lut.elements.as_deref();
    while let Some(st) = e {
        elems.push(st);
        e = st.next.as_deref();
    }
    let elem_count = elems.len() as u32;
    if !io::write_u16(io, in_ch as u16)
        || !io::write_u16(io, out_ch as u16)
        || !io::write_u32(io, elem_count)
    {
        return false;
    }
    let dir_pos = io.tell();
    for _ in 0..elem_count {
        if !io::write_u32(io, 0) || !io::write_u32(io, 0) {
            return false;
        }
    }
    let mut offsets = vec![0u32; elems.len()];
    let mut sizes = vec![0u32; elems.len()];
    for (i, st) in elems.iter().enumerate() {
        offsets[i] = io.tell().wrapping_sub(base_offset);
        let elem_sig = st.stage_type();
        let Some(h) = mpe_handler(elem_sig, s.ctx) else {
            err::signal_error(None, ERROR_UNKNOWN_EXTENSION, "Found unknown MPE type");
            return false;
        };
        let before = io.tell();
        if !io::write_u32(io, elem_sig) || !io::write_u32(io, 0) {
            return false;
        }
        if h.foreign != 0 {
            // Foreign element — `WritePtr(self, io, Ptr, n)`; the C `Ptr` is
            // the `cmsStage*`, and plugins read `mpe->Data` — hand them the
            // ForeignEval `data` pointer.
            let Some(hooks) = *crate::context::TAGTYPE_HOOKS.read().unwrap() else {
                return false;
            };
            let ptr = match &st.data {
                crate::lut::StageData::ForeignEval { data, .. } => *data,
                _ => *st as *const crate::lut::Stage as usize,
            };
            if !(hooks.write)(
                h.foreign,
                s.icc_version,
                io as *mut IoHandler as usize,
                ptr,
                1,
            ) || !io::write_alignment(io)
            {
                return false;
            }
            sizes[i] = io.tell().wrapping_sub(before);
            continue;
        }
        let Some(write) = h.write else {
            return false;
        };
        if !write(s, io, &TagObject::MpeElem((*st).clone()), 1) || !io::write_alignment(io) {
            return false;
        }
        sizes[i] = io.tell().wrapping_sub(before);
    }
    let cur_pos = io.tell();
    if !io.seek(dir_pos) {
        return false;
    }
    for i in 0..elems.len() {
        if !io::write_u32(io, offsets[i]) || !io::write_u32(io, sizes[i]) {
            return false;
        }
    }
    io.seek(cur_pos)
}

// ---------------------------------------------------------------------------
// vcgt / VideoSignal (cicp) / MHC2 / Dictionary
// ---------------------------------------------------------------------------

const VIDEO_CARD_GAMMA_TABLE_TYPE: u32 = 0;
const VIDEO_CARD_GAMMA_FORMULA_TYPE: u32 = 1;

/// `Type_vcgt_Read` — returns a triple of tone curves
fn type_vcgt_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let mut tag_type = 0u32;
    if !io::read_u32(io, &mut tag_type) {
        return None;
    }
    let mut curves: [Option<ToneCurve>; 3] = [None, None, None];

    match tag_type {
        VIDEO_CARD_GAMMA_TABLE_TYPE => {
            let (mut nch, mut nelems, mut nbytes) = (0u16, 0u16, 0u16);
            if !io::read_u16(io, &mut nch) {
                return None;
            }
            if nch != 3 {
                err::signal_error(
                    None,
                    ERROR_UNKNOWN_EXTENSION,
                    "Unsupported number of channels for VCGT",
                );
                return None;
            }
            if !io::read_u16(io, &mut nelems) || !io::read_u16(io, &mut nbytes) {
                return None;
            }
            // Adobe's quirk fixup — broken profiles
            if nelems == 256 && nbytes == 1 && size_of_tag == 1576 {
                nbytes = 2;
            }
            for c in curves.iter_mut() {
                let Some(tc) = build_tabulated_tone_curve16(None, nelems as u32, None) else {
                    return None;
                };
                let mut tc = tc;
                match nbytes {
                    1 => {
                        for i in 0..nelems as usize {
                            let mut v = 0u8;
                            if !io::read_u8(io, &mut v) {
                                return None;
                            }
                            tc.table16[i] = from_8_to_16(v);
                        }
                    }
                    2 => {
                        if !io::read_u16_array(io, nelems as u32, Some(&mut tc.table16)) {
                            return None;
                        }
                    }
                    _ => {
                        err::signal_error(
                            None,
                            ERROR_UNKNOWN_EXTENSION,
                            "Unsupported bit depth for VCGT",
                        );
                        return None;
                    }
                }
                *c = Some(tc);
            }
        }
        VIDEO_CARD_GAMMA_FORMULA_TYPE => {
            for c in curves.iter_mut() {
                let (mut gamma, mut min, mut max) = (0.0f64, 0.0f64, 0.0f64);
                if !io::read_15_fixed16(io, &mut gamma)
                    || !io::read_15_fixed16(io, &mut min)
                    || !io::read_15_fixed16(io, &mut max)
                {
                    return None;
                }
                // Parametric type 5: a = (Max-Min)^(1/Gamma), e = Min
                let mut params = [0.0f64; 10];
                params[0] = gamma;
                params[1] = (max - min).cpow(1.0 / gamma);
                params[5] = min;
                let Some(tc) = build_parametric_tone_curve(None, 5, &params) else {
                    return None;
                };
                *c = Some(tc);
            }
        }
        _ => {
            err::signal_error(
                None,
                ERROR_UNKNOWN_EXTENSION,
                "Unsupported tag type for VCGT",
            );
            return None;
        }
    }
    *n_items = 1;
    let c = curves.map(|c| c.unwrap());
    let _ = s;
    Some(TagObject::Vcgt(VcgtData::Curves(c)))
}

/// `Type_vcgt_Write` — formula iff all 3 are parametric-5, else 256×u16 table
fn type_vcgt_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Vcgt(VcgtData::Curves(curves)) = p else {
        return false;
    };
    if curves.iter().all(|c| c.parametric_type() == 5) {
        if !io::write_u32(io, VIDEO_CARD_GAMMA_FORMULA_TYPE) {
            return false;
        }
        for c in curves.iter() {
            let gamma = c.segments[0].params[0];
            let min = c.segments[0].params[5];
            let max = c.segments[0].params[1].cpow(gamma) + min;
            if !io::write_15_fixed16(io, gamma)
                || !io::write_15_fixed16(io, min)
                || !io::write_15_fixed16(io, max)
            {
                return false;
            }
        }
    } else {
        if !io::write_u32(io, VIDEO_CARD_GAMMA_TABLE_TYPE)
            || !io::write_u16(io, 3)
            || !io::write_u16(io, 256)
            || !io::write_u16(io, 2)
        {
            return false;
        }
        for c in curves.iter() {
            for j in 0..256u32 {
                let v = c.eval_float(j as f32 / 255.0);
                let n = crate::fixed::quick_saturate_word(v as f64 * 65535.0);
                if !io::write_u16(io, n) {
                    return false;
                }
            }
        }
    }
    true
}

/// `Type_VideoSignal_Read` — 'cicp', exactly 4 bytes
fn type_video_signal_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    if size_of_tag != 4 {
        return None;
    }
    let (mut cp, mut tc, mut mc, mut fr) = (0u8, 0u8, 0u8, 0u8);
    if !io::read_u8(io, &mut cp)
        || !io::read_u8(io, &mut tc)
        || !io::read_u8(io, &mut mc)
        || !io::read_u8(io, &mut fr)
    {
        return None;
    }
    *n_items = 1;
    Some(TagObject::VideoSignal(VideoSignal {
        colour_primaries: cp,
        transfer_characteristics: tc,
        matrix_coefficients: mc,
        video_full_range_flag: fr,
    }))
}

fn type_video_signal_write(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    p: &TagObject,
    _n: u32,
) -> bool {
    let TagObject::VideoSignal(v) = p else {
        return false;
    };
    io::write_u8(io, v.colour_primaries)
        && io::write_u8(io, v.transfer_characteristics)
        && io::write_u8(io, v.matrix_coefficients)
        && io::write_u8(io, v.video_full_range_flag)
}

const MHC2_IDENTITY: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn mhc2_is_identity(m: &[[f64; 4]; 3]) -> bool {
    (0..3).all(|i| (0..4).all(|j| (m[i][j] - MHC2_IDENTITY[i][j]).abs() < 1.0 / 65535.0))
}

/// `Type_MHC2_Read` — Microsoft MHC2
fn type_mhc2_read(
    _s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    _sz: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let mut mhc2 = Mhc2 {
        curve_entries: 0,
        red_curve: Vec::new(),
        green_curve: Vec::new(),
        blue_curve: Vec::new(),
        min_luminance: 0.0,
        peak_luminance: 0.0,
        xyz2xyz_matrix: MHC2_IDENTITY,
    };
    if !io::read_u32(io, &mut mhc2.curve_entries) {
        return None;
    }
    if mhc2.curve_entries > 4096 {
        return None;
    }
    mhc2.red_curve = vec![0.0; mhc2.curve_entries as usize];
    mhc2.green_curve = vec![0.0; mhc2.curve_entries as usize];
    mhc2.blue_curve = vec![0.0; mhc2.curve_entries as usize];
    if !io::read_15_fixed16(io, &mut mhc2.min_luminance)
        || !io::read_15_fixed16(io, &mut mhc2.peak_luminance)
    {
        return None;
    }
    let (mut moff, mut roff, mut goff, mut boff) = (0u32, 0u32, 0u32, 0u32);
    if !io::read_u32(io, &mut moff)
        || !io::read_u32(io, &mut roff)
        || !io::read_u32(io, &mut goff)
        || !io::read_u32(io, &mut boff)
    {
        return None;
    }
    if moff != 0 {
        let at = base_offset.wrapping_add(moff);
        let cur = io.tell();
        if !io.seek(at) {
            return None;
        }
        for row in &mut mhc2.xyz2xyz_matrix {
            for v in row {
                if !io::read_15_fixed16(io, v) {
                    return None;
                }
            }
        }
        if !io.seek(cur) {
            return None;
        }
    }
    // Each table = sf32 sig + filler (8B) + doubles
    for (off, tab) in [
        (roff, &mut mhc2.red_curve),
        (goff, &mut mhc2.green_curve),
        (boff, &mut mhc2.blue_curve),
    ] {
        let at = base_offset.wrapping_add(off).wrapping_add(8);
        let cur = io.tell();
        if !io.seek(at) {
            return None;
        }
        for v in tab.iter_mut() {
            if !io::read_15_fixed16(io, v) {
                return None;
            }
        }
        if !io.seek(cur) {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::Mhc2(mhc2))
}

fn write_15_fixed16_list(io: &mut IoHandler, v: &[f64]) -> bool {
    v.iter().all(|&x| io::write_15_fixed16(io, x))
}

/// `Type_MHC2_Write`
fn type_mhc2_write(_s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Mhc2(m) = p else { return false };
    let base_offset = io.tell().wrapping_sub(8);
    if !io::write_u32(io, m.curve_entries)
        || !io::write_15_fixed16(io, m.min_luminance)
        || !io::write_15_fixed16(io, m.peak_luminance)
    {
        return false;
    }
    let tables_pos = io.tell();
    for _ in 0..4 {
        if !io::write_u32(io, 0) {
            return false;
        }
    }
    let moff = if mhc2_is_identity(&m.xyz2xyz_matrix) {
        0
    } else {
        let off = io.tell().wrapping_sub(base_offset);
        for v in m.xyz2xyz_matrix.iter().flatten() {
            if !io::write_15_fixed16(io, *v) {
                return false;
            }
        }
        off
    };
    let mut curve_offsets = [0u32; 3];
    for (i, tab) in [&m.red_curve, &m.green_curve, &m.blue_curve]
        .iter()
        .enumerate()
    {
        curve_offsets[i] = io.tell().wrapping_sub(base_offset);
        if !io::write_u32(io, tsig::SigS15Fixed16ArrayType)
            || !io::write_u32(io, 0)
            || !write_15_fixed16_list(io, tab)
        {
            return false;
        }
    }
    if !io.seek(tables_pos) {
        return false;
    }
    if !io::write_u32(io, moff)
        || !io::write_u32(io, curve_offsets[0])
        || !io::write_u32(io, curve_offsets[1])
        || !io::write_u32(io, curve_offsets[2])
    {
        return false;
    }
    true
}

// --- Dictionary ('dict') ---------------------------------------------------

/// `_cmsDICelem` — parallel offset/size column
#[derive(Default, Clone)]
struct DicElem {
    offsets: Vec<u32>,
    sizes: Vec<u32>,
}

fn read_one_elem(io: &mut IoHandler, e: &mut DicElem, i: usize, base_offset: u32) -> bool {
    if !io::read_u32(io, &mut e.offsets[i]) || !io::read_u32(io, &mut e.sizes[i]) {
        return false;
    }
    if e.offsets[i] > 0 {
        e.offsets[i] = e.offsets[i].wrapping_add(base_offset);
    }
    true
}

fn write_one_elem(io: &mut IoHandler, e: &DicElem, i: usize) -> bool {
    io::write_u32(io, e.offsets[i]) && io::write_u32(io, e.sizes[i])
}

/// `Type_Dictionary_Read`
fn type_dictionary_read(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    n_items: &mut u32,
    size_of_tag: u32,
) -> Option<TagObject> {
    *n_items = 0;
    let base_offset = io.tell().wrapping_sub(8);
    let mut signed_size = size_of_tag as i32;

    signed_size -= 4;
    if signed_size < 0 {
        return None;
    }
    let mut count = 0u32;
    if !io::read_u32(io, &mut count) {
        return None;
    }
    signed_size -= 4;
    if signed_size < 0 {
        return None;
    }
    let mut length = 0u32;
    if !io::read_u32(io, &mut length) {
        return None;
    }
    if length != 16 && length != 24 && length != 32 {
        err::signal_error(
            None,
            ERROR_UNKNOWN_EXTENSION,
            "Unknown record length in dictionary",
        );
        return None;
    }

    let mut dict = crate::named::dict_alloc(None);
    // column arrays
    let mut name = DicElem {
        offsets: vec![0; count as usize],
        sizes: vec![0; count as usize],
    };
    let mut value = name.clone();
    let mut display_name = name.clone();
    let mut display_value = name.clone();
    let _ = (&mut display_name, &mut display_value); // length-gated below

    for i in 0..count as usize {
        if signed_size < 16 {
            return None;
        }
        signed_size -= 16;
        if !read_one_elem(io, &mut name, i, base_offset)
            || !read_one_elem(io, &mut value, i, base_offset)
        {
            return None;
        }
        if length > 16 {
            if signed_size < 8 {
                return None;
            }
            signed_size -= 8;
            if !read_one_elem(io, &mut display_name, i, base_offset) {
                return None;
            }
        }
        if length > 24 {
            if signed_size < 8 {
                return None;
            }
            signed_size -= 8;
            if !read_one_elem(io, &mut display_value, i, base_offset) {
                return None;
            }
        }
    }

    for i in 0..count as usize {
        // ReadOneWChar
        let name_w = read_dic_wchar(io, &name, i)?;
        let value_w = read_dic_wchar(io, &value, i)?;
        let dn = if length > 16 {
            read_dic_mlu(s, io, &display_name, i)?
        } else {
            None
        };
        let dv = if length > 24 {
            read_dic_mlu(s, io, &display_value, i)?
        } else {
            None
        };
        let rc = match (&name_w, &value_w) {
            (Some(n), Some(v)) => crate::named::dict_add_entry(
                &mut dict,
                n,
                Some(v),
                dn.as_ref(),
                dv.as_ref(),
            ),
            _ => {
                err::signal_error(None, ERROR_CORRUPTION_DETECTED, "Bad dictionary Name/Value");
                false
            }
        };
        if !rc {
            return None;
        }
    }
    *n_items = 1;
    Some(TagObject::Dictionary(dict))
}

fn read_dic_wchar(io: &mut IoHandler, e: &DicElem, i: usize) -> Option<Option<Vec<Wchar>>> {
    if e.offsets[i] == 0 {
        return Some(None); // undefined string
    }
    if !io.seek(e.offsets[i]) {
        return None;
    }
    let n_chars = e.sizes[i] / 2;
    if n_chars > 0x7ffff {
        return None;
    }
    let mut wchars = Vec::new();
    if !read_wchar_array(io, n_chars, &mut wchars) {
        return None;
    }
    Some(Some(wchars))
}

fn read_dic_mlu(
    s: &TagTypeHandler,
    io: &mut IoHandler,
    e: &DicElem,
    i: usize,
) -> Option<Option<Mlu>> {
    if e.offsets[i] == 0 || e.sizes[i] == 0 {
        return Some(None);
    }
    if !io.seek(e.offsets[i]) {
        return None;
    }
    let mut n = 0u32;
    match type_mlu_read(s, io, &mut n, e.sizes[i]) {
        Some(TagObject::Mlu(m)) => Some(Some(m)),
        _ => None,
    }
}

/// `mywcslen` — slice up to (not including) the first NUL.
fn wcs_body(v: &[Wchar]) -> &[Wchar] {
    let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
    &v[..n]
}

/// `Type_Dictionary_Write`
fn type_dictionary_write(s: &TagTypeHandler, io: &mut IoHandler, p: &TagObject, _n: u32) -> bool {
    let TagObject::Dictionary(dict) = p else {
        return false;
    };
    let base_offset = io.tell().wrapping_sub(8);

    let count = dict.entries.len() as u32;
    let any_name = dict.entries.iter().any(|e| e.display_name.is_some());
    let any_value = dict.entries.iter().any(|e| e.display_value.is_some());
    let length = 16 + 8 * any_name as u32 + 8 * any_value as u32;

    if !io::write_u32(io, count) || !io::write_u32(io, length) {
        return false;
    }
    let dir_pos = io.tell();

    let mk = || DicElem {
        offsets: vec![0; count as usize],
        sizes: vec![0; count as usize],
    };
    let (mut name, mut value, mut dn, mut dv) = (mk(), mk(), mk(), mk());

    // fake directory
    for i in 0..count as usize {
        if !write_one_elem(io, &name, i) || !write_one_elem(io, &value, i) {
            return false;
        }
        if length > 16 && !write_one_elem(io, &dn, i) {
            return false;
        }
        if length > 24 && !write_one_elem(io, &dv, i) {
            return false;
        }
    }

    // Write each element
    for (i, entry) in dict.entries.iter().enumerate() {
        // `WriteOneWChar` — writes `mywcslen` wchars (no NUL); NULL → offset 0.
        let before = io.tell();
        name.offsets[i] = before.wrapping_sub(base_offset);
        if !write_wchar_array(io, wcs_body(&entry.name)) {
            return false;
        }
        name.sizes[i] = io.tell().wrapping_sub(before);

        match &entry.value {
            None => {
                value.offsets[i] = 0;
                value.sizes[i] = 0;
            }
            Some(v) => {
                let before = io.tell();
                value.offsets[i] = before.wrapping_sub(base_offset);
                if !write_wchar_array(io, wcs_body(v)) {
                    return false;
                }
                value.sizes[i] = io.tell().wrapping_sub(before);
            }
        }

        if let Some(m) = &entry.display_name {
            let before = io.tell();
            dn.offsets[i] = before.wrapping_sub(base_offset);
            if !type_mlu_write(s, io, &TagObject::Mlu(m.clone()), 1) {
                return false;
            }
            dn.sizes[i] = io.tell().wrapping_sub(before);
        }
        if let Some(m) = &entry.display_value {
            let before = io.tell();
            dv.offsets[i] = before.wrapping_sub(base_offset);
            if !type_mlu_write(s, io, &TagObject::Mlu(m.clone()), 1) {
                return false;
            }
            dv.sizes[i] = io.tell().wrapping_sub(before);
        }
    }

    let cur = io.tell();
    if !io.seek(dir_pos) {
        return false;
    }
    for i in 0..count as usize {
        if !write_one_elem(io, &name, i) || !write_one_elem(io, &value, i) {
            return false;
        }
        if length > 16 && !write_one_elem(io, &dn, i) {
            return false;
        }
        if length > 24 && !write_one_elem(io, &dv, i) {
            return false;
        }
    }
    io.seek(cur)
}

// ---------------------------------------------------------------------------
// Tag registry — `SupportedTags` + `_cmsGetTagDescriptor` + `GetHandler`
// ---------------------------------------------------------------------------

use crate::signatures::tag as tagsig;

/// `DecideLUTtypeA2B`
fn decide_lut_a2b(v: f64, d: &TagObject) -> u32 {
    let (TagObject::LutA2B(lut)
    | TagObject::Lut8(lut)
    | TagObject::Lut16(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Mpe(lut)) = d
    else {
        return 0; // type-mismatched payload — is_type_supported rejects
    };
    if v < 4.0 {
        if lut.save_as_8bits {
            tsig::SigLut8Type
        } else {
            tsig::SigLut16Type
        }
    } else {
        tsig::SigLutAtoBType
    }
}

/// `DecideLUTtypeB2A`
fn decide_lut_b2a(v: f64, d: &TagObject) -> u32 {
    let (TagObject::LutA2B(lut)
    | TagObject::Lut8(lut)
    | TagObject::Lut16(lut)
    | TagObject::LutB2A(lut)
    | TagObject::Mpe(lut)) = d
    else {
        return 0; // type-mismatched payload — is_type_supported rejects
    };
    if v < 4.0 {
        if lut.save_as_8bits {
            tsig::SigLut8Type
        } else {
            tsig::SigLut16Type
        }
    } else {
        tsig::SigLutBtoAType
    }
}

/// `cmsTagDescriptor` — supported types for a tag + type selection
#[derive(Clone, Copy)]
pub struct TagDescriptor {
    /// `ElemCount` — how many items the tag expects (read check)
    pub elem_count: u32,
    /// `SupportedTypes` — allowed type signatures (order preserved)
    pub supported_types: &'static [u32],
    /// `DecideType` — pick the write type for an ICC version + object
    pub decide_type: Option<fn(f64, &TagObject) -> u32>,
}

const LUT_AB: &[u32] = &[tsig::SigLut16Type, tsig::SigLutAtoBType, tsig::SigLut8Type];
const LUT_BA: &[u32] = &[tsig::SigLut16Type, tsig::SigLutBtoAType, tsig::SigLut8Type];
const XYZ_BROKEN: &[u32] = &[tsig::SigXYZType, 0x17A505B8];
const TRC3: &[u32] = &[tsig::SigCurveType, tsig::SigParametricCurveType, 0x9478ee00];
const TRC2: &[u32] = &[tsig::SigCurveType, tsig::SigParametricCurveType];
const ONE_DATETIME: &[u32] = &[tsig::SigDateTimeType];
const ONE_TEXT: &[u32] = &[tsig::SigTextType];
const ONE_S15: &[u32] = &[tsig::SigS15Fixed16ArrayType];
const ONE_CHRM: &[u32] = &[tsig::SigChromaticityType];
const ONE_CLRT: &[u32] = &[tsig::SigColorantOrderType];
const ONE_CLRTAB: &[u32] = &[tsig::SigColorantTableType];
const TEXT3: &[u32] = &[
    tsig::SigTextType,
    tsig::SigMultiLocalizedUnicodeType,
    tsig::SigTextDescriptionType,
];
const DESC3: &[u32] = &[
    tsig::SigTextDescriptionType,
    tsig::SigMultiLocalizedUnicodeType,
    tsig::SigTextType,
];
const ONE_NCLR: &[u32] = &[tsig::SigNamedColor2Type];
const ONE_PSEQ: &[u32] = &[tsig::SigProfileSequenceDescType];
const ONE_SIG: &[u32] = &[tsig::SigSignatureType];
const ONE_MEAS: &[u32] = &[tsig::SigMeasurementType];
const ONE_DATA: &[u32] = &[tsig::SigDataType];
const ONE_UCRBG: &[u32] = &[tsig::SigUcrBgType];
const ONE_CRD: &[u32] = &[tsig::SigCrdInfoType];
const ONE_MPE: &[u32] = &[tsig::SigMultiProcessElementType];
const ONE_DESC: &[u32] = &[tsig::SigTextDescriptionType];
const ONE_VIEW: &[u32] = &[tsig::SigViewingConditionsType];
const ONE_SCRN: &[u32] = &[tsig::SigScreeningType];
const ONE_VCGT: &[u32] = &[tsig::SigVcgtType];
const ONE_DICT: &[u32] = &[tsig::SigDictType];
const ONE_PSID: &[u32] = &[tsig::SigProfileSequenceIdType];
const ONE_MLU: &[u32] = &[tsig::SigMultiLocalizedUnicodeType];
const ONE_CICP: &[u32] = &[tsig::SigcicpType];
const ONE_MHC2: &[u32] = &[tsig::SigMHC2Type];
const ONE_XYZ: &[u32] = &[tsig::SigXYZType];

/// `SupportedTags` — same order as C
pub static SUPPORTED_TAGS: &[(u32, TagDescriptor)] = &[
    (
        tagsig::SigAToB0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_AB,
            decide_type: Some(decide_lut_a2b),
        },
    ),
    (
        tagsig::SigAToB1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_AB,
            decide_type: Some(decide_lut_a2b),
        },
    ),
    (
        tagsig::SigAToB2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_AB,
            decide_type: Some(decide_lut_a2b),
        },
    ),
    (
        tagsig::SigBToA0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigBToA1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigBToA2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigRedColorantTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: XYZ_BROKEN,
            decide_type: Some(decide_xyz_type),
        },
    ),
    (
        tagsig::SigGreenColorantTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: XYZ_BROKEN,
            decide_type: Some(decide_xyz_type),
        },
    ),
    (
        tagsig::SigBlueColorantTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: XYZ_BROKEN,
            decide_type: Some(decide_xyz_type),
        },
    ),
    (
        tagsig::SigRedTRCTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: TRC3,
            decide_type: Some(decide_curve_type),
        },
    ),
    (
        tagsig::SigGreenTRCTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: TRC3,
            decide_type: Some(decide_curve_type),
        },
    ),
    (
        tagsig::SigBlueTRCTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: TRC3,
            decide_type: Some(decide_curve_type),
        },
    ),
    (
        tagsig::SigCalibrationDateTimeTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATETIME,
            decide_type: None,
        },
    ),
    (
        tagsig::SigCharTargetTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_TEXT,
            decide_type: None,
        },
    ),
    (
        tagsig::SigChromaticAdaptationTag,
        TagDescriptor {
            elem_count: 9,
            supported_types: ONE_S15,
            decide_type: None,
        },
    ),
    (
        tagsig::SigChromaticityTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CHRM,
            decide_type: None,
        },
    ),
    (
        tagsig::SigColorantOrderTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CLRT,
            decide_type: None,
        },
    ),
    (
        tagsig::SigColorantTableTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CLRTAB,
            decide_type: None,
        },
    ),
    (
        tagsig::SigColorantTableOutTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CLRTAB,
            decide_type: None,
        },
    ),
    (
        tagsig::SigCopyrightTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: TEXT3,
            decide_type: Some(decide_text_type),
        },
    ),
    (
        tagsig::SigDateTimeTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATETIME,
            decide_type: None,
        },
    ),
    (
        tagsig::SigDeviceMfgDescTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: DESC3,
            decide_type: Some(decide_text_desc_type),
        },
    ),
    (
        tagsig::SigDeviceModelDescTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: DESC3,
            decide_type: Some(decide_text_desc_type),
        },
    ),
    (
        tagsig::SigGamutTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigGrayTRCTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: TRC2,
            decide_type: Some(decide_curve_type),
        },
    ),
    (
        tagsig::SigLuminanceTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_XYZ,
            decide_type: None,
        },
    ),
    (
        tagsig::SigMediaBlackPointTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: XYZ_BROKEN,
            decide_type: None,
        },
    ),
    (
        tagsig::SigMediaWhitePointTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: XYZ_BROKEN,
            decide_type: None,
        },
    ),
    (
        tagsig::SigNamedColor2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_NCLR,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPreview0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigPreview1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigPreview2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: LUT_BA,
            decide_type: Some(decide_lut_b2a),
        },
    ),
    (
        tagsig::SigProfileDescriptionTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: DESC3,
            decide_type: Some(decide_text_desc_type),
        },
    ),
    (
        tagsig::SigProfileSequenceDescTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_PSEQ,
            decide_type: None,
        },
    ),
    (
        tagsig::SigTechnologyTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_SIG,
            decide_type: None,
        },
    ),
    (
        tagsig::SigColorimetricIntentImageStateTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_SIG,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPerceptualRenderingIntentGamutTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_SIG,
            decide_type: None,
        },
    ),
    (
        tagsig::SigSaturationRenderingIntentGamutTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_SIG,
            decide_type: None,
        },
    ),
    (
        tagsig::SigMeasurementTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MEAS,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2CRD0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2CRD1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2CRD2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2CRD3Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2CSATag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigPs2RenderingIntentTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DATA,
            decide_type: None,
        },
    ),
    (
        tagsig::SigViewingCondDescTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: DESC3,
            decide_type: Some(decide_text_desc_type),
        },
    ),
    (
        tagsig::SigUcrBgTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_UCRBG,
            decide_type: None,
        },
    ),
    (
        tagsig::SigCrdInfoTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CRD,
            decide_type: None,
        },
    ),
    (
        tagsig::SigDToB0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigDToB1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigDToB2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigDToB3Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigBToD0Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigBToD1Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigBToD2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigBToD3Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MPE,
            decide_type: None,
        },
    ),
    (
        tagsig::SigScreeningDescTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DESC,
            decide_type: None,
        },
    ),
    (
        tagsig::SigViewingConditionsTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_VIEW,
            decide_type: None,
        },
    ),
    (
        tagsig::SigScreeningTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_SCRN,
            decide_type: None,
        },
    ),
    (
        tagsig::SigVcgtTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_VCGT,
            decide_type: None,
        },
    ),
    (
        tagsig::SigMetaTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_DICT,
            decide_type: None,
        },
    ),
    (
        tagsig::SigProfileSequenceIdTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_PSID,
            decide_type: None,
        },
    ),
    (
        tagsig::SigProfileDescriptionMLTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MLU,
            decide_type: None,
        },
    ),
    (
        tagsig::SigcicpTag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_CICP,
            decide_type: None,
        },
    ),
    (
        tagsig::SigArgyllArtsTag,
        TagDescriptor {
            elem_count: 9,
            supported_types: ONE_S15,
            decide_type: None,
        },
    ),
    (
        tagsig::SigMHC2Tag,
        TagDescriptor {
            elem_count: 1,
            supported_types: ONE_MHC2,
            decide_type: None,
        },
    ),
];

/// `GetHandler` — plugin list first, then built-ins
pub fn get_handler<'a>(
    sig: u32,
    plugins: &'a [TagTypeHandler],
    defaults: &'a [TagTypeHandler],
) -> Option<&'a TagTypeHandler> {
    plugins
        .iter()
        .find(|h| h.signature == sig)
        .or_else(|| defaults.iter().find(|h| h.signature == sig))
}

/// `_cmsGetTagDescriptor` — plugin tags first, then SupportedTags.
/// Returns an owned copy (`TagDescriptor` is `Copy`; C returns a pointer
/// into the plugin list or the static table — equivalent for dispatch).
pub fn get_tag_descriptor(ctx: Option<&Context>, sig: u32) -> Option<TagDescriptor> {
    if let Some(c) = ctx {
        if let Some((_, d)) = c.tag_descriptors.iter().find(|(s, _)| *s == sig) {
            return Some(*d);
        }
    }
    SUPPORTED_TAGS
        .iter()
        .find(|(s, _)| *s == sig)
        .map(|(_, d)| *d)
}

/// `_cmsAvoidTypeCheckOnTags` — plugin registered signature-0 → skip checks
pub fn avoid_type_check_on_tags(ctx: Option<&Context>) -> bool {
    ctx.map(|c| c.tag_descriptors.iter().any(|(s, _)| *s == 0))
        .unwrap_or(false)
}
