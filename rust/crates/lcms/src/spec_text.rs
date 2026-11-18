//! ICC.1 encodings for localized text and the containers that embed it.

use crate::named::{Dict, DictEntry, Mlu, MluEntry, ProfileSeqDesc, alloc_profile_seq_desc};
use crate::tagtypes::TagObject;
use std::collections::BTreeSet;

pub(super) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn word(out: &mut Vec<u8>, n: usize) -> Option<()> {
    out.extend(u32::try_from(n).ok()?.to_be_bytes());
    Some(())
}

fn set_word(out: &mut [u8], at: usize, n: usize) -> Option<()> {
    out.get_mut(at..at.checked_add(4)?)?
        .copy_from_slice(&u32::try_from(n).ok()?.to_be_bytes());
    Some(())
}

fn align(out: &mut Vec<u8>) {
    out.resize((out.len() + 3) & !3, 0);
}

fn type_base(kind: &[u8; 4]) -> Vec<u8> {
    [kind.as_slice(), &[0; 4]].concat()
}

fn has_base(data: &[u8], kind: &[u8; 4]) -> bool {
    data.get(..8)
        .is_some_and(|h| h[..4] == kind[..] && h[4..] == [0; 4])
}

fn from_utf16(data: &[u8]) -> Option<Vec<i32>> {
    if data.len() % 2 != 0 {
        return None;
    }
    char::decode_utf16(
        data.chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]])),
    )
    .map(|r| r.ok().map(|c| c as i32))
    .collect()
}

fn put_utf16(out: &mut Vec<u8>, chars: &[i32]) -> Option<()> {
    for &value in chars {
        for unit in char::from_u32(value as u32)?.encode_utf16(&mut [0; 2]) {
            out.extend(unit.to_be_bytes());
        }
    }
    Some(())
}

fn append_record(mlu: &mut Mlu, locale: [u8; 4], chars: &[i32]) -> Option<()> {
    let str_w = u32::try_from(mlu.pool.len()).ok()?;
    for c in chars {
        mlu.pool.extend(c.to_ne_bytes());
    }
    let end = u32::try_from(mlu.pool.len()).ok()?;
    mlu.entries.push(MluEntry {
        language: u16::from_be_bytes([locale[0], locale[1]]),
        country: u16::from_be_bytes([locale[2], locale[3]]),
        str_w,
        len: end - str_w,
    });
    mlu.used_entries = u32::try_from(mlu.entries.len()).ok()?;
    mlu.allocated_entries = mlu.used_entries;
    mlu.pool_used = end;
    Some(())
}

fn records(mlu: &Mlu) -> Option<Vec<([u8; 4], Vec<i32>)>> {
    mlu.entries
        .get(..mlu.used_entries as usize)?
        .iter()
        .map(|e| {
            let start = e.str_w as usize;
            let len = e.len as usize;
            let mut chars = mlu
                .pool
                .get(start..start.checked_add(len)?)?
                .chunks_exact(4)
                .map(|c| i32::from_ne_bytes(c.try_into().unwrap()))
                .collect::<Vec<_>>();
            // The in-memory MLU uses a zero unit to represent an empty string.
            if chars == [0] {
                chars.clear();
            }
            let [a, b] = e.language.to_be_bytes();
            let [c, d] = e.country.to_be_bytes();
            Some(([a, b, c, d], chars))
        })
        .collect()
}

fn locale_is_well_formed([a, b, c, d]: [u8; 4]) -> bool {
    ([a, b] == [0, 0] || a.is_ascii_lowercase() && b.is_ascii_lowercase())
        && ([c, d] == [0, 0] || c.is_ascii_uppercase() && d.is_ascii_uppercase())
}

/// Table 54. Measure every offset in encoded bytes, including surrogate pairs.
fn read_mluc(data: &[u8]) -> Option<(Mlu, usize)> {
    if !has_base(data, b"mluc") || u32_at(data, 12)? != 12 {
        return None;
    }
    let count = u32_at(data, 8)? as usize;
    let storage = 16usize.checked_add(count.checked_mul(12)?)?;
    let entries = data.get(16..storage)?;
    let mut end = storage;
    let mut mlu = Mlu::default();
    for entry in entries.chunks_exact(12) {
        let len = u32_at(entry, 4)? as usize;
        let at = u32_at(entry, 8)? as usize;
        if at < storage || at % 2 != 0 {
            return None;
        }
        let stop = at.checked_add(len)?;
        end = end.max(stop);
        let mut locale: [u8; 4] = entry[..4].try_into().ok()?;
        // Tolerate legacy Little CMS files that wrote its v2 sentinel.
        if locale == [255; 4] {
            locale = [0; 4];
        }
        if !locale_is_well_formed(locale) {
            return None;
        }
        let chars = from_utf16(data.get(at..stop)?)?;
        append_record(&mut mlu, locale, &chars)?;
    }
    Some((mlu, end))
}

fn write_mluc(mlu: Option<&Mlu>) -> Option<Vec<u8>> {
    let records = mlu.map(records).unwrap_or_else(|| Some(Vec::new()))?;
    let mut out = type_base(b"mluc");
    word(&mut out, records.len())?;
    word(&mut out, 12)?;
    out.resize(16usize.checked_add(records.len().checked_mul(12)?)?, 0);
    for (i, (mut locale, chars)) in records.into_iter().enumerate() {
        if locale == [255; 4] {
            locale = [0; 4];
        }
        if !locale_is_well_formed(locale) {
            return None;
        }
        let entry = 16 + i * 12;
        let start = out.len();
        out[entry..entry + 4].copy_from_slice(&locale);
        set_word(&mut out, entry + 8, start)?;
        put_utf16(&mut out, &chars)?;
        let size = out.len() - start;
        set_word(&mut out, entry + 4, size)?;
    }
    Some(out)
}

fn read_desc(data: &[u8]) -> Option<(Mlu, usize)> {
    if !has_base(data, b"desc") {
        return None;
    }
    let ascii_end = 12usize.checked_add(u32_at(data, 8)? as usize)?;
    let ascii_bytes = data.get(12..ascii_end)?;
    let nul = ascii_bytes.iter().position(|b| *b == 0)?;
    let ascii = &ascii_bytes[..nul];
    if !ascii.is_ascii() || ascii_bytes[nul..].iter().any(|&b| b != 0) {
        return None;
    }
    let unicode_at = ascii_end.checked_add(8)?;
    let unicode_end =
        unicode_at.checked_add((u32_at(data, ascii_end + 4)? as usize).checked_mul(2)?)?;
    let end = unicode_end.checked_add(70)?;
    let script = data.get(unicode_end..end)?;
    if script[2] != 0 || u32_at(data, ascii_end)? != 0 {
        // Localized v2 language codes and ScriptCode have no lossless Mlu representation.
        return None;
    }
    let mut mlu = Mlu::default();
    let ascii: Vec<_> = ascii.iter().map(|b| i32::from(*b)).collect();
    append_record(&mut mlu, *b"enUS", &ascii)?;
    if unicode_at < unicode_end {
        let unicode = from_utf16(data.get(unicode_at..unicode_end)?)?;
        let nul = unicode.iter().position(|&c| c == 0)?;
        if unicode[nul..].iter().any(|&c| c != 0) {
            return None;
        }
        if unicode[..nul] != ascii {
            append_record(&mut mlu, [255; 4], &unicode[..nul])?;
        }
    }
    Some((mlu, end))
}

fn write_desc(mlu: Option<&Mlu>) -> Option<Vec<u8>> {
    let records = mlu.map(records).unwrap_or_else(|| Some(Vec::new()))?;
    let mut ascii = Vec::new();
    let mut unicode = None;
    match records.as_slice() {
        [] => {}
        [(locale, chars)] if locale_is_well_formed(*locale) => {
            if chars.iter().all(|&c| (1..128).contains(&c)) {
                ascii.extend(chars.iter().map(|&c| c as u8));
            } else {
                unicode = Some(chars.as_slice());
            }
        }
        // The two representations of a single legacy description.
        [(ascii_locale, invariant), (unicode_locale, localized)]
            if (*ascii_locale == *b"enUS" || *ascii_locale == [0; 4])
                && (*unicode_locale == [255; 4] || *unicode_locale == [0; 4])
                && invariant.iter().all(|&c| (1..128).contains(&c)) =>
        {
            ascii.extend(invariant.iter().map(|&c| c as u8));
            unicode = Some(localized.as_slice());
        }
        _ => return None,
    }
    let mut out = type_base(b"desc");
    word(&mut out, ascii.len() + 1)?;
    out.extend(ascii);
    out.push(0);
    word(&mut out, 0)?; // no Unicode language code
    let count_at = out.len();
    word(&mut out, 0)?;
    if let Some(chars) = unicode {
        if chars.contains(&0) {
            return None;
        }
        put_utf16(&mut out, chars)?;
        out.extend([0; 2]);
        let units = (out.len() - count_at - 4) / 2;
        set_word(&mut out, count_at, units)?;
    }
    out.extend([0; 70]); // ScriptCode fields and fixed-size storage
    Some(out)
}

fn read_pseq(data: &[u8], v4: bool) -> Option<ProfileSeqDesc> {
    let count = u32_at(data, 8)?;
    let min_size = if v4 { 52 } else { 202 };
    if data.len().checked_sub(12)? / min_size < count as usize {
        return None;
    }
    let mut seq = alloc_profile_seq_desc(None, count)?;
    let mut at = 12;
    for item in &mut seq.seq {
        item.device_mfg = u32_at(data, at)?;
        item.device_model = u32_at(data, at + 4)?;
        item.attributes = u64::from_be_bytes(data.get(at + 8..at + 16)?.try_into().ok()?);
        item.technology = u32_at(data, at + 16)?;
        at += 20;
        for slot in [&mut item.manufacturer, &mut item.model] {
            let read = if v4 { read_mluc } else { read_desc };
            let (mlu, size) = read(data.get(at..)?)?;
            *slot = Some(mlu);
            at = at.checked_add(size)?;
            if !v4 {
                // Older Little CMS writes padding inside the embedded desc.
                let padded = at + ((4 - size % 4) % 4);
                if data
                    .get(at..padded)
                    .is_some_and(|pad| pad.iter().all(|&b| b == 0))
                    && (padded == data.len() || has_base(&data[padded..], b"desc"))
                {
                    at = padded;
                }
            }
        }
    }
    (at == data.len()).then_some(seq)
}

fn write_pseq(seq: &ProfileSeqDesc, v4: bool) -> Option<Vec<u8>> {
    let mut out = type_base(b"pseq");
    word(&mut out, seq.seq.len())?;
    for item in &seq.seq {
        out.extend(item.device_mfg.to_be_bytes());
        out.extend(item.device_model.to_be_bytes());
        out.extend(item.attributes.to_be_bytes());
        out.extend(item.technology.to_be_bytes());
        let encode = if v4 { write_mluc } else { write_desc };
        out.extend(encode(item.manufacturer.as_ref())?);
        out.extend(encode(item.model.as_ref())?);
    }
    Some(out)
}

fn read_psid(data: &[u8]) -> Option<ProfileSeqDesc> {
    let count = u32_at(data, 8)?;
    let table_end = 12usize.checked_add((count as usize).checked_mul(8)?)?;
    let table = data.get(12..table_end)?;
    let mut seq = alloc_profile_seq_desc(None, count)?;
    for (item, pos) in seq.seq.iter_mut().zip(table.chunks_exact(8)) {
        let at = u32_at(pos, 0)? as usize;
        let len = u32_at(pos, 4)? as usize;
        if at < table_end || len < 32 {
            return None;
        }
        let d = data.get(at..at.checked_add(len)?)?;
        item.profile_id.id8.copy_from_slice(&d[..16]);
        item.description = Some(read_mluc(&d[16..])?.0);
    }
    Some(seq)
}

fn write_psid(seq: &ProfileSeqDesc) -> Option<Vec<u8>> {
    let mut out = type_base(b"psid");
    word(&mut out, seq.seq.len())?;
    out.resize(12usize.checked_add(seq.seq.len().checked_mul(8)?)?, 0);
    for (i, item) in seq.seq.iter().enumerate() {
        let at = out.len();
        out.extend(item.profile_id.id8);
        out.extend(write_mluc(item.description.as_ref())?);
        let len = out.len() - at;
        set_word(&mut out, 12 + i * 8, at)?;
        set_word(&mut out, 16 + i * 8, len)?;
        align(&mut out);
    }
    Some(out)
}

fn read_dict(data: &[u8]) -> Option<Dict> {
    let count = u32_at(data, 8)? as usize;
    let width = u32_at(data, 12)? as usize;
    if !matches!(width, 16 | 24 | 32) {
        return None;
    }
    let store = 16usize.checked_add(count.checked_mul(width)?)?;
    let mut dict = Dict::default();
    let mut names = BTreeSet::new();
    for record in data.get(16..store)?.chunks_exact(width) {
        let item = |i: usize| -> Option<Option<&[u8]>> {
            if i * 8 >= width {
                return Some(None);
            }
            let at = u32_at(record, i * 8)? as usize;
            let size = u32_at(record, i * 8 + 4)? as usize;
            if at == 0 {
                return Some(None);
            }
            if at < store || at % 4 != 0 {
                return None;
            }
            Some(Some(data.get(at..at.checked_add(size)?)?))
        };
        let name = from_utf16(item(0)??)?;
        if name.is_empty() || name.contains(&0) || !names.insert(name.clone()) {
            return None;
        }
        let value = item(1)?.map_or(Some(None), |b| from_utf16(b).map(Some))?;
        if value.as_ref().is_some_and(|v| v.contains(&0)) {
            return None;
        }
        let mluc = |i| item(i)?.map_or(Some(None), |b| read_mluc(b).map(|(m, _)| Some(m)));
        dict.entries.push(DictEntry {
            name,
            value,
            display_name: mluc(2)?,
            display_value: mluc(3)?,
        });
    }
    Some(dict)
}

fn write_dict(dict: &Dict) -> Option<Vec<u8>> {
    let width = if dict.entries.iter().any(|e| e.display_value.is_some()) {
        32
    } else if dict.entries.iter().any(|e| e.display_name.is_some()) {
        24
    } else {
        16
    };
    let mut out = type_base(b"dict");
    word(&mut out, dict.entries.len())?;
    word(&mut out, width)?;
    out.resize(
        16usize.checked_add(dict.entries.len().checked_mul(width)?)?,
        0,
    );
    let mut names = BTreeSet::new();
    for (i, entry) in dict.entries.iter().enumerate() {
        let name = entry.name.strip_suffix(&[0]).unwrap_or(&entry.name);
        if name.is_empty() || name.contains(&0) || !names.insert(name) {
            return None;
        }
        let mut element = |field: usize, encode: &mut dyn FnMut(&mut Vec<u8>) -> Option<()>| {
            let at = out.len();
            encode(&mut out)?;
            let size = out.len() - at;
            let record = 16 + i * width + field * 8;
            set_word(&mut out, record, at)?;
            set_word(&mut out, record + 4, size)?;
            align(&mut out);
            Some(())
        };
        element(0, &mut |out| put_utf16(out, name))?;
        if let Some(v) = &entry.value {
            let v = v.strip_suffix(&[0]).unwrap_or(v);
            if v.contains(&0) {
                return None;
            }
            element(1, &mut |out| put_utf16(out, v))?;
        }
        for (field, mlu) in [(2, &entry.display_name), (3, &entry.display_value)] {
            if let Some(mlu) = mlu {
                element(field, &mut |out| {
                    out.extend(write_mluc(Some(mlu))?);
                    Some(())
                })?;
            }
        }
    }
    Some(out)
}

pub(super) fn decode(kind: u32, bytes: &[u8], v4: bool) -> Result<Option<TagObject>, &'static str> {
    let parsed = match &kind.to_be_bytes() {
        b"desc" => read_desc(bytes).map(|(m, _)| TagObject::TextDescription(m)),
        b"mluc" => read_mluc(bytes).map(|(m, _)| TagObject::Mlu(m)),
        b"pseq" => read_pseq(bytes, v4).map(TagObject::ProfileSeqDesc),
        b"psid" => read_psid(bytes).map(TagObject::ProfileSeqId),
        b"dict" => read_dict(bytes).map(TagObject::Dictionary),
        _ => return Ok(None),
    };
    parsed
        .map(Some)
        .ok_or("invalid or unsupported ICC text, sequence or dictionary data")
}

pub(super) fn encode(
    kind: u32,
    object: &TagObject,
    v4: bool,
) -> Result<Option<Vec<u8>>, &'static str> {
    use TagObject::{Mlu, Text, TextDescription};
    let mlu = || match object {
        Mlu(m) | Text(m) | TextDescription(m) => Some(m),
        _ => None,
    };
    let seq = || match object {
        TagObject::ProfileSeqDesc(s) | TagObject::ProfileSeqId(s) => Some(s),
        _ => None,
    };
    let encoded = match &kind.to_be_bytes() {
        b"mluc" => mlu().and_then(|m| write_mluc(Some(m))),
        b"desc" => mlu().and_then(|m| write_desc(Some(m))),
        b"text" => mlu().and_then(|m| {
            let records = records(m)?;
            let [(_, chars)] = records.as_slice() else {
                return None;
            };
            if !chars.iter().all(|&c| (1..128).contains(&c)) {
                return None;
            }
            let mut out = type_base(b"text");
            out.extend(chars.iter().map(|&c| c as u8));
            out.push(0);
            Some(out)
        }),
        b"pseq" => seq().and_then(|s| write_pseq(s, v4)),
        b"psid" if v4 => seq().and_then(write_psid),
        b"dict" if v4 => match object {
            TagObject::Dictionary(d) => write_dict(d),
            _ => None,
        },
        _ => return Ok(None),
    };
    encoded
        .map(Some)
        .ok_or("ICC version cannot represent this text, sequence or dictionary payload")
}
