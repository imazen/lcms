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
    BehaviorMode::of(ctx) == BehaviorMode::SpecExact
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
    })
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
) -> Result<TagObject, &'static str> {
    let kind = handler.signature;
    if bytes.get(4..8) != Some(&[0; 4]) {
        return Err("nonzero tag reserved field");
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
        return Ok(obj);
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
    Ok(obj)
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
    let matrix = all(&[b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC"]) && pcs == *b"XYZ ";
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
            if linked < 0 || icc.tag_ptrs[linked as usize].is_none() {
                return refuse("known ICC tag has no decoded representation");
            }
            if !desc
                .supported_types
                .contains(&icc.tag_type_sigs[linked as usize])
            {
                return refuse("linked ICC tags require incompatible types");
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
