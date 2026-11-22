//! ICC.1 policy at the profile I/O and linking boundaries.

use crate::api::BehaviorMode;
use crate::context::Context;
use crate::io::{self, IoHandler};
use crate::lut::{Pipeline, StageData};
use crate::profile::{self, IccProfile};
use crate::spec_text::{self, u32_at};
use crate::tagtypes::{TagObject, TagTypeHandler, VideoSignal, get_tag_descriptor};
use crate::types::{ERROR_CORRUPTION_DETECTED, ERROR_NOT_SUITABLE, ProfileId};

pub(crate) fn enabled(ctx: Option<&Context>) -> bool {
    BehaviorMode::of(ctx) != BehaviorMode::Lcms2Exact
}

pub(crate) fn refuse(reason: &str) -> bool {
    crate::err::signal_error(None, ERROR_NOT_SUITABLE, reason);
    false
}

pub(crate) fn bad_tag(sig: u32, reason: &str) {
    let bytes = sig.to_be_bytes();
    let tag = String::from_utf8_lossy(&bytes);
    crate::err::signal_error(None, ERROR_CORRUPTION_DETECTED, &format!("{tag}: {reason}"));
}

pub(crate) fn decode_known_tags(icc: &mut IccProfile) -> bool {
    let tags: Vec<_> = icc.tag_names[..icc.tag_count as usize].to_vec();
    tags.into_iter().all(|sig| {
        get_tag_descriptor(icc.ctx.as_ref(), sig).is_none()
            || profile::cms_read_tag(icc, sig).is_some()
            || is_opaque_mpe(icc, sig)
    })
}

pub(crate) fn is_opaque_mpe(icc: &IccProfile, sig: u32) -> bool {
    let Ok(i) = usize::try_from(profile::search_tag(icc, sig, true)) else {
        return false;
    };
    icc.tag_save_as_raw[i]
        && icc.tag_type_sigs[i] == u32::from_be_bytes(*b"mpet")
        && icc.tag_offsets[i] != 0
}

// ICC.1 §10.16 defines the framing even for an undefined element type.
// Validate it before allowing the complete tag to be carried as opaque data.
fn mpe_has_undefined_element(bytes: &[u8]) -> Option<bool> {
    let channels = |data: &[u8], at: usize| {
        let n = u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?);
        (n != 0).then_some(n)
    };
    let mut input = channels(bytes, 8)?;
    let output = channels(bytes, 10)?;
    let n = u32_at(bytes, 12)? as usize;
    if n == 0 {
        return None;
    }
    let start = 16usize.checked_add(n.checked_mul(8)?)?;
    let mut undefined = false;
    let positions = bytes.get(16..start)?;
    let mut spans = Vec::new();
    spans.try_reserve_exact(n).ok()?;
    for position in positions.chunks_exact(8) {
        let offset = u32_at(position, 0)? as usize;
        let size = u32_at(position, 4)? as usize;
        if offset < start || offset % 4 != 0 || size < 12 {
            return None;
        }
        let end = offset.checked_add(size)?;
        let element = bytes.get(offset..end)?;
        if element[4..8] != [0; 4] || channels(element, 8)? != input {
            return None;
        }
        input = channels(element, 10)?;
        undefined |= !matches!(&element[..4], b"cvst" | b"matf" | b"clut");
        spans.push((offset, end));
    }
    spans.sort_unstable();
    spans.dedup();
    let contiguous = spans.windows(2).all(|p| {
        let (end, next) = (p[0].1, p[1].0);
        bytes
            .get(end..next)
            .is_some_and(|pad| pad.len() <= 3 && pad.iter().all(|&v| v == 0))
    });
    if input != output || spans[0].0 != start || !contiguous {
        return None;
    }
    Some(undefined)
}

pub(crate) fn bind_curves(object: &mut TagObject, kind: u32) -> bool {
    if kind == u32::from_be_bytes(*b"mpet") {
        return true;
    }
    match object {
        TagObject::Curve(c) | TagObject::ParametricCurve(c) => c.bind_icc(),
        TagObject::Lut8(p) | TagObject::Lut16(p) | TagObject::LutA2B(p) | TagObject::LutB2A(p) => {
            let mut stage = p.elements.as_deref_mut();
            while let Some(s) = stage {
                if let StageData::ToneCurves(curves) = &mut s.data {
                    if !curves.iter_mut().all(|c| c.bind_icc()) {
                        return false;
                    }
                }
                stage = s.next.as_deref_mut();
            }
            true
        }
        _ => true,
    }
}

/// Keep the C tag readers inside the tag's declared byte extent.
pub(crate) fn decode_tag(
    icc: &IccProfile,
    handler: &TagTypeHandler,
    bytes: &[u8],
    count: &mut u32,
) -> Result<Option<TagObject>, &'static str> {
    let kind = handler.signature;
    if bytes.get(4..8) != Some(&[0; 4]) {
        return Err("nonzero tag reserved field");
    }
    if kind == u32::from_be_bytes(*b"mpet")
        && mpe_has_undefined_element(bytes).ok_or("invalid MPE element framing")?
    {
        return Ok(None);
    }
    let obj = if &kind.to_be_bytes() == b"cicp" {
        Some(TagObject::VideoSignal(
            read_cicp(icc, bytes).ok_or("invalid CICP encoding or profile placement")?,
        ))
    } else {
        spec_text::decode(kind, bytes, icc.version >= 0x04000000)?
    };
    if let Some(obj) = obj {
        *count = 1;
        return Ok(Some(obj));
    }
    let mut io =
        io::io_handler_from_mem(None, bytes.to_vec(), "r").ok_or("cannot open tag data")?;
    if !io.seek(8) {
        return Err("missing type header");
    }
    let mut obj = (handler.read)(handler, &mut io, count, bytes.len() as u32 - 8)
        .ok_or("invalid or unsupported tag encoding")?;
    if !bind_curves(&mut obj, kind) {
        return Err("undefined ICC curve function");
    }
    if let Some(lut) = pipeline_mut(&mut obj) {
        let eight_bit = match &kind.to_be_bytes() {
            b"mft1" => true,
            b"mAB " | b"mBA " => {
                let off = u32_at(bytes, 24).unwrap_or(0) as usize;
                off != 0 && off.checked_add(16).and_then(|n| bytes.get(n)) == Some(&1)
            }
            _ => false,
        };
        lut.set_save_as_8bits(eight_bit);
    }
    Ok(Some(obj))
}

pub(crate) fn read_cicp(icc: &IccProfile, bytes: &[u8]) -> Option<VideoSignal> {
    if bytes.get(..8)? != b"cicp\0\0\0\0" || !matches!(bytes.len(), 12 | 44) {
        return None;
    }
    let signal = VideoSignal {
        colour_primaries: bytes[8],
        transfer_characteristics: bytes[9],
        matrix_coefficients: bytes[10],
        video_full_range_flag: bytes[11],
    };
    let extended = bytes.len() == 44;
    (icc.version >= 0x04400000
        && matches!(&icc.device_class.to_be_bytes(), b"scnr" | b"mntr" | b"spac")
        && (matches!(&icc.color_space.to_be_bytes(), b"RGB " | b"YCbr")
            || !extended && &icc.color_space.to_be_bytes() == b"XYZ ")
        && signal.video_full_range_flag <= 1
        && (signal.matrix_coefficients != 0) == (&icc.color_space.to_be_bytes() == b"YCbr")
        && (!extended || signal.colour_primaries == 2))
        .then_some(signal)
}

pub(crate) fn cicp_chromaticities(icc: &IccProfile) -> Option<[i32; 8]> {
    let index =
        usize::try_from(profile::search_tag(icc, u32::from_be_bytes(*b"cicp"), true)).ok()?;
    let bytes = icc.tag_raw[index].as_deref()?;
    let ext = bytes.get(12..44)?;
    Some(std::array::from_fn(|i| {
        i32::from_be_bytes(ext[4 * i..4 * i + 4].try_into().unwrap())
    }))
}

pub(crate) fn encode_cicp(signal: VideoSignal, chromaticities: Option<[i32; 8]>) -> Vec<u8> {
    let mut bytes = b"cicp\0\0\0\0".to_vec();
    bytes.extend([
        signal.colour_primaries,
        signal.transfer_characteristics,
        signal.matrix_coefficients,
        signal.video_full_range_flag,
    ]);
    if let Some(coords) = chromaticities {
        bytes.extend(coords.into_iter().flat_map(i32::to_be_bytes));
    }
    bytes
}

fn pipeline_mut(object: &mut TagObject) -> Option<&mut Pipeline> {
    match object {
        TagObject::Lut8(p)
        | TagObject::Lut16(p)
        | TagObject::LutA2B(p)
        | TagObject::LutB2A(p)
        | TagObject::Mpe(p) => Some(p),
        _ => None,
    }
}

fn add_identity_grid(lut: &mut Pipeline) -> bool {
    if lut.input_channels != lut.output_channels || !(1..=15).contains(&lut.input_channels) {
        return false;
    }
    let Some(grid) = crate::lut::stage_alloc_identity_clut(None, lut.input_channels) else {
        return false;
    };
    // The grid goes between the two optional curve sets.
    let mut cursor = &mut lut.elements;
    while cursor
        .as_ref()
        .is_some_and(|stage| matches!(stage.data, StageData::Matrix { .. }))
    {
        cursor = &mut cursor.as_mut().unwrap().next;
    }
    if cursor
        .as_ref()
        .is_some_and(|s| matches!(s.data, StageData::ToneCurves(_)))
    {
        cursor = &mut cursor.as_mut().unwrap().next;
    }
    let mut grid = Box::new(grid);
    grid.next = cursor.take();
    *cursor = Some(grid);
    true
}

fn check_curve(curve: &crate::gamma::ToneCurve) -> bool {
    !curve.has_exact_inverse_representation()
        && curve.icc_function_is_defined()
        && (!curve.segments.is_empty() || curve.table16.len() >= 2)
        && curve.parameters().is_none_or(|p| {
            p.iter()
                .all(|v| (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&(v * 65536.0)))
        })
}

fn check_mpe_curve(curve: &crate::gamma::ToneCurve) -> bool {
    let segs = &curve.segments;
    let formula = |s: &crate::gamma::CurveSegment| (6..=8).contains(&s.r#type);
    !curve.behavior_is_spec()
        && !segs.is_empty()
        && segs.len() <= usize::from(u16::MAX)
        && formula(&segs[0])
        && formula(&segs[segs.len() - 1])
        && segs
            .windows(2)
            .all(|w| w[0].x1.is_finite() && w[0].x1 == w[1].x0 && w[0].x0 < w[0].x1)
        && segs.iter().all(|s| {
            if formula(s) {
                s.params.iter().all(|&p| (p as f32).is_finite())
            } else {
                s.r#type == 0
                    && s.n_grid_points >= 2
                    && s.sampled_points.as_ref().is_some_and(|points| {
                        points.len() == s.n_grid_points as usize
                            && points.iter().all(|p| p.is_finite())
                    })
            }
        })
}

fn validate_pipeline(lut: &Pipeline, kind: &[u8; 4]) -> bool {
    let legacy = matches!(kind, b"mft1" | b"mft2");
    if lut.elements.is_none() {
        return false;
    }
    let mut stage = lut.elements.as_deref();
    while let Some(s) = stage {
        match &s.data {
            StageData::ToneCurves(curves) if kind == b"mpet" => {
                if curves.len() != s.output_channels as usize || !curves.iter().all(check_mpe_curve)
                {
                    return false;
                }
            }
            StageData::ToneCurves(curves) => {
                if curves.is_empty() || curves.iter().any(|c| !check_curve(c)) {
                    return false;
                }
                if kind == b"mft2" {
                    let n = curves[0].table16.len();
                    if !(2..=4096).contains(&n) || curves.iter().any(|c| c.table16.len() != n) {
                        return false;
                    }
                }
            }
            StageData::CLut(c) if kind != b"mpet" => {
                let grids = &c.n_samples[..c.n_inputs as usize];
                if c.tab16.is_none()
                    || grids.iter().any(|n| !(2..=255).contains(n))
                    || legacy && grids.windows(2).any(|w| w[0] != w[1])
                {
                    return false;
                }
            }
            _ => {}
        }
        stage = s.next.as_deref();
    }
    true
}

fn pipeline_dimensions(icc: &IccProfile, sig: u32, input: u32, output: u32) -> bool {
    let device = crate::pcs::channels_of_color_space(icc.color_space) as u32;
    let pcs = crate::pcs::channels_of_color_space(icc.pcs) as u32;
    let expected = match &sig.to_be_bytes() {
        b"A2B0" | b"A2B1" | b"A2B2" | b"D2B0" | b"D2B1" | b"D2B2" | b"D2B3" => (device, pcs),
        b"B2A0" | b"B2A1" | b"B2A2" | b"B2D0" | b"B2D1" | b"B2D2" | b"B2D3" => (pcs, device),
        b"pre0" | b"pre1" | b"pre2" => (pcs, pcs),
        b"gamt" => (pcs, 1),
        _ => return true,
    };
    expected == (input, output)
}

fn invertible_chad(values: &[f64]) -> bool {
    let Ok(values) = <&[f64; 9]>::try_from(values) else {
        return false;
    };
    if values
        .iter()
        .any(|&v| !(-32768.0..=i32::MAX as f64 / 65536.0).contains(&v))
    {
        return false;
    }
    // Use the exact determinant of the values that the writer will encode.
    let [a, b, c, d, e, f, g, h, i] =
        values.map(|v| i128::from(crate::fixed::double_to_s15_fixed16(v)));
    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g) != 0
}

/// Encode only the operations implemented here and use the existing handlers
/// for common byte layouts.
pub(crate) fn write_tag(
    icc: &IccProfile,
    sig: u32,
    handler: &TagTypeHandler,
    io: &mut IoHandler,
    mut obj: TagObject,
    count: u32,
) -> bool {
    let kind = handler.signature.to_be_bytes();
    if kind == *b"cicp" {
        let TagObject::VideoSignal(signal) = obj else {
            return refuse("CICP requires video-signal data");
        };
        let bytes = encode_cicp(signal, cicp_chromaticities(icc));
        return (read_cicp(icc, &bytes).is_some() && io.write(bytes.len() as u32, &bytes))
            || refuse("invalid CICP encoding or profile placement");
    }
    let v4 = icc.version >= 0x04000000;
    match spec_text::encode(handler.signature, &obj, v4) {
        Ok(Some(bytes)) => return io.write(bytes.len() as u32, &bytes),
        Err(reason) => return refuse(reason),
        Ok(None) => {}
    }
    match &mut obj {
        TagObject::Curve(c) | TagObject::ParametricCurve(c) if !check_curve(c) => {
            return refuse("ICC curve cannot represent the selected function");
        }
        _ => {}
    }
    if kind == *b"curv" {
        if let TagObject::ParametricCurve(c) = obj {
            obj = TagObject::Curve(c);
        }
    }
    if let Some(lut) = pipeline_mut(&mut obj) {
        let has_clut = std::iter::successors(lut.elements.as_deref(), |s| s.next.as_deref())
            .any(|s| matches!(s.data, StageData::CLut(_)));
        if matches!(&kind, b"mft1" | b"mft2") && !has_clut && !add_identity_grid(lut) {
            return refuse("ICC legacy LUT requires a CLUT");
        }
        if matches!(&kind, b"mft1" | b"mft2") {
            if let Some(StageData::Matrix { double, offset }) = lut.first_stage().map(|s| &s.data) {
                let input = if sig.to_be_bytes().starts_with(b"A2B") {
                    icc.color_space
                } else {
                    icc.pcs
                };
                if (input != u32::from_be_bytes(*b"XYZ ")
                    && double != &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
                    || offset.as_ref().is_some_and(|v| v.iter().any(|&x| x != 0.0))
                {
                    return refuse(
                        "legacy ICC LUT matrix requires a PCSXYZ input and has no offset",
                    );
                }
            }
        }
        if !validate_pipeline(lut, &kind) {
            return refuse("pipeline cannot be represented in the selected ICC LUT encoding");
        }
    }
    if !io::write_type_base(io, handler.signature) || !(handler.write)(handler, io, &obj, count) {
        return refuse(&format!(
            "ICC tag '{}' has no supported encoding",
            String::from_utf8_lossy(&sig.to_be_bytes())
        ));
    }
    true
}

fn version_allows(sig: u32, v4: bool) -> bool {
    match &sig.to_be_bytes() {
        b"B2D0" | b"B2D1" | b"B2D2" | b"B2D3" | b"D2B0" | b"D2B1" | b"D2B2" | b"D2B3" | b"cicp"
        | b"psid" | b"HAGC" | b"meta" | b"clrt" | b"clot" | b"clro" | b"ciis" | b"rig0"
        | b"rig2" => v4,
        b"bkpt" | b"crdi" | b"scrd" | b"devs" | b"ncol" | b"psd0" | b"psd1" | b"psd2" | b"psd3"
        | b"ps2s" | b"ps2i" | b"scrn" | b"bfd " => !v4,
        _ => true,
    }
}

pub(crate) fn allowed_version(version: u32) -> bool {
    matches!(version >> 24, 2 | 4)
        && version & 0xffff == 0
        && version >> 20 & 0xf <= 4
        && version >> 16 & 0xf <= 9
}

pub(crate) fn check_export(icc: &IccProfile) -> bool {
    let v4 = icc.version >= 0x04000000;
    let has = |tag: &[u8; 4]| profile::cms_is_tag(icc, u32::from_be_bytes(*tag));
    let all = |tags: &[&[u8; 4]]| tags.iter().all(|tag| has(tag));
    let class = icc.device_class.to_be_bytes();
    let pcs = icc.pcs.to_be_bytes();
    let color = icc.color_space.to_be_bytes();
    if !allowed_version(icc.version) {
        return refuse("only ICC.1 v2 and v4 profile exports are supported");
    }
    if !all(&[b"desc", b"cprt"]) || class != *b"link" && !has(b"wtpt") {
        return refuse("ICC profile is missing required description, copyright or white point");
    }
    let matrix = all(&[b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC"])
        && pcs == *b"XYZ "
        && crate::pcs::channels_of_color_space(icc.color_space) == 3;
    let gray = color == *b"GRAY" && has(b"kTRC");
    let xclr = |cs: [u8; 4]| &cs[1..] == b"CLR";
    let valid = match &class {
        b"scnr" => has(b"A2B0") || matrix || gray,
        b"mntr" => all(&[b"A2B0", b"B2A0"]) || matrix || gray,
        b"prtr" => {
            gray || all(&[
                b"A2B0", b"A2B1", b"A2B2", b"B2A0", b"B2A1", b"B2A2", b"gamt",
            ]) && (!v4 || !xclr(color) || has(b"clrt"))
        }
        b"link" => {
            all(&[b"A2B0", b"pseq"])
                && (!v4 || ((!xclr(color) || has(b"clrt")) && (!xclr(pcs) || has(b"clot"))))
        }
        b"spac" => all(&[b"A2B0", b"B2A0"]),
        b"abst" => has(b"A2B0"),
        b"nmcl" => has(b"ncl2"),
        _ => false,
    };
    if !valid {
        return refuse(
            "ICC profile class is missing a required model or sequence (internal profiles cannot be exported)",
        );
    }
    if v4 && class == *b"mntr" {
        let is_d50 = match profile::read_tag_frozen(icc, u32::from_be_bytes(*b"wtpt")) {
            Some(TagObject::Xyz(white)) => {
                let scale = |v: f64| (v * 10000.0).round();
                [scale(white.X), scale(white.Y), scale(white.Z)] == [9642.0, 10000.0, 8249.0]
            }
            _ => false,
        };
        if !is_d50 {
            return refuse("ICC v4 Display media white point must equal the PCS illuminant");
        }
    }

    for (i, &sig) in icc.tag_names[..icc.tag_count as usize].iter().enumerate() {
        if sig == 0 {
            continue;
        }
        if !version_allows(sig, v4) {
            return refuse(&format!(
                "tag '{}' is unavailable in the selected ICC version",
                String::from_utf8_lossy(&sig.to_be_bytes())
            ));
        }
        if &sig.to_be_bytes() == b"clot" && class != *b"link" {
            return refuse("colorantTableOutTag requires a DeviceLink profile");
        }
        if let Some(desc) = get_tag_descriptor(icc.ctx.as_ref(), sig) {
            let linked = profile::search_tag(icc, sig, true);
            if linked < 0 || (icc.tag_ptrs[linked as usize].is_none() && !is_opaque_mpe(icc, sig)) {
                return refuse("known ICC tag has no decoded representation");
            }
            if !desc
                .supported_types
                .contains(&icc.tag_type_sigs[linked as usize])
            {
                return refuse("linked ICC tags require incompatible types");
            }
            if &sig.to_be_bytes() == b"chad"
                && !matches!(
                    &icc.tag_ptrs[linked as usize], Some(TagObject::S15Fixed16(v)) if invertible_chad(v)
                )
            {
                return refuse(
                    "ICC chromatic adaptation matrix must encode nine invertible values",
                );
            }
            let dimensions = if is_opaque_mpe(icc, sig) {
                let bytes = icc.tag_raw[linked as usize].as_ref().unwrap();
                Some((
                    u32::from(u16::from_be_bytes([bytes[8], bytes[9]])),
                    u32::from(u16::from_be_bytes([bytes[10], bytes[11]])),
                ))
            } else {
                match &icc.tag_ptrs[linked as usize] {
                    Some(
                        TagObject::Lut8(l)
                        | TagObject::Lut16(l)
                        | TagObject::LutA2B(l)
                        | TagObject::LutB2A(l)
                        | TagObject::Mpe(l),
                    ) => Some((l.input_channels, l.output_channels)),
                    _ => None,
                }
            };
            if dimensions
                .is_some_and(|(input, output)| !pipeline_dimensions(icc, sig, input, output))
            {
                return refuse("ICC LUT channel counts disagree with the profile colour spaces");
            }
        } else if icc.tag_linked[i] == 0 && icc.tag_ptrs[i].is_none() {
            // Unknown types are opaque. Their bytes are carried by the source
            // stream or by a raw write in the low-level API.
            if (icc.io.is_none() || icc.tag_offsets[i] == 0) && icc.tag_raw[i].is_none() {
                return refuse("opaque ICC tag has no payload");
            }
        }
    }
    true
}

// Compact ICC Profiles stores other tags over the unused ScriptCode bytes
// of the v2 description. Recognize the complete text and the zeroed headers
// for the unused representations before replacing only that description.
pub(crate) fn expand_compact_v2_description(data: &[u8]) -> Option<Vec<u8>> {
    let version = u32_at(data, 8)?;
    if u32_at(data, 0)? as usize != data.len() || !allowed_version(version) || version >> 24 != 2 {
        return None;
    }
    let count = u32_at(data, 128)? as usize;
    let table_end = 132usize.checked_add(count.checked_mul(12)?)?;
    let entries = data.get(132..table_end)?;
    let mut records = entries
        .chunks_exact(12)
        .enumerate()
        .filter(|(_, r)| &r[..4] == b"desc");
    let (i, record) = records.next()?;
    if records.next().is_some() {
        return None;
    }
    let offset = u32_at(record, 4)? as usize;
    let declared = u32_at(record, 8)? as usize;
    let desc = data.get(offset..offset.checked_add(declared)?)?;
    if offset < table_end || offset % 4 != 0 || desc.get(..8)? != b"desc\0\0\0\0" {
        return None;
    }
    let ascii_end = 12usize.checked_add(u32_at(desc, 8)? as usize)?;
    let full_size = ascii_end.checked_add(78)?;
    let prefix_end = ascii_end.checked_add(14)? & !3;
    // These profiles declare either the full textDescriptionType size or
    // one byte less. The shared tail begins exactly after the zero headers.
    if !matches!(full_size.checked_sub(declared), Some(0 | 1))
        || desc.get(ascii_end..prefix_end)?.iter().any(|&b| b != 0)
    {
        return None;
    }
    let ascii = desc.get(12..ascii_end)?.strip_suffix(&[0])?;
    if ascii.is_empty() || !ascii.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return None;
    }
    let next = offset.checked_add(prefix_end)?;
    let mut first_after = usize::MAX;
    for (index, entry) in entries.chunks_exact(12).enumerate() {
        if index == i {
            continue;
        }
        let start = u32_at(entry, 4)? as usize;
        let stop = start.checked_add(u32_at(entry, 8)? as usize)?;
        // Check against the original extent and the part of the description
        // that contains text. Appending storage must never legalize either.
        if stop > data.len() || (start < next && offset < stop) {
            return None;
        }
        if start > offset {
            first_after = first_after.min(start);
        }
    }
    if first_after != next {
        return None;
    }

    let start = data.len().checked_add(3)? & !3;
    let end = start.checked_add(full_size)?.checked_add(3)? & !3;
    let end_u32 = u32::try_from(end).ok()?;
    let mut expanded = bytemuck::try_zeroed_vec::<u8>(end).ok()?;
    expanded[..data.len()].copy_from_slice(data);
    expanded[start..start + ascii_end].copy_from_slice(&desc[..ascii_end]);
    expanded[..4].copy_from_slice(&end_u32.to_be_bytes());
    let at = 132 + i * 12;
    expanded[at + 4..at + 8].copy_from_slice(&(start as u32).to_be_bytes());
    expanded[at + 8..at + 12].copy_from_slice(&(full_size as u32).to_be_bytes());
    // The ordinary ICC reader checks all remaining spans and decodes their
    // contents. The expanded backing buffer is internal to the reader.
    Some(expanded)
}

pub(crate) fn validate_directory(data: &[u8]) -> bool {
    let valid = || {
        let size = u32_at(data, 0)? as usize;
        let version = u32_at(data, 8)?;
        if size != data.len() || !allowed_version(version) {
            return None;
        }
        let count = u32_at(data, 128)? as usize;
        let end = 132usize.checked_add(count.checked_mul(12)?)?;
        let entries = data.get(132..end)?;
        let mut spans = Vec::new();
        for rec in entries.chunks_exact(12) {
            let sig = u32_at(rec, 0)?;
            let start = u32_at(rec, 4)? as usize;
            let len = u32_at(rec, 8)? as usize;
            if sig == 0 || start < end || start % 4 != 0 || len < 8 {
                return None;
            }
            let stop = start.checked_add(len)?;
            data.get(start..stop)?;
            spans.push((start, stop));
        }
        spans.sort_unstable();
        spans.dedup(); // shared tag blocks
        if spans.windows(2).any(|p| p[0].1 > p[1].0) {
            return None;
        }
        Some(())
    };
    if valid().is_none() {
        crate::err::signal_error(
            None,
            ERROR_CORRUPTION_DETECTED,
            "invalid ICC header or tag directory",
        );
        return false;
    }
    true
}

pub(crate) fn digest(bytes: &[u8]) -> ProfileId {
    let mut md5 = crate::md5::Md5::new();
    md5.add(&bytes[..44]);
    md5.add(&[0; 4]);
    md5.add(&bytes[48..64]);
    md5.add(&[0; 4]);
    md5.add(&bytes[68..84]);
    md5.add(&[0; 16]);
    md5.add(&bytes[100..]);
    ProfileId { id8: md5.finish() }
}
