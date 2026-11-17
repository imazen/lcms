//! An ICC profile checker written from the specification texts, not from
//! lcms. It parses the raw bytes itself and reports every rule a profile
//! breaks, each finding citing the clause it comes from.
//!
//! Sources (imazen-private/standards-ark, `icc/`; tables were checked
//! against the PDF pages, since the Markdown extracts drop symbols):
//! - ICC.1:2022-05, profile version 4.4.0.0: clauses 4, 6.3.4, 7, 8, 9, 10.
//! - ICC.1:2001-04, profile version 2.4.0: clauses 6.1 to 6.5.
//! - ICC.1 amendment "cicpType: interpretation of ColourPrimaries value 2",
//!   approved 2026-10-19: an optional 32-byte extension when
//!   ColourPrimaries is 2.
//! - ICC.1 amendment "Headroom Adaptive Gain Curve" (approved 2026-10-19):
//!   the optional 'HAGC' tag of type 'hagc'.
//!
//! The checker only reports. Which findings are lcms2 behaviour (the
//! C oracle produces them too) and which would be port bugs is decided by
//! the tests that call it.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// How binding the broken rule is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// A "shall" requirement.
    Shall,
    /// A "should" recommendation.
    Should,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub level: Level,
    /// Clause, e.g. "ICC.1:2022 7.3.4".
    pub clause: &'static str,
    /// The tag the finding is about, if any.
    pub tag: Option<String>,
    pub msg: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lvl = match self.level {
            Level::Shall => "shall",
            Level::Should => "should",
        };
        match &self.tag {
            Some(t) => write!(f, "[{lvl}] {} '{t}': {}", self.clause, self.msg),
            None => write!(f, "[{lvl}] {}: {}", self.clause, self.msg),
        }
    }
}

pub fn sig(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub fn sig_str(v: u32) -> String {
    v.to_be_bytes()
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn s15f16(b: &[u8], o: usize) -> Option<f64> {
    be32(b, o).map(|v| v as i32 as f64 / 65536.0)
}
fn f32be(b: &[u8], o: usize) -> Option<f32> {
    be32(b, o).map(f32::from_bits)
}
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Number of channels a Table 19 colour space signature implies, or `None`
/// if it is not a Table 19 signature.
pub fn channels(cs: u32) -> Option<u32> {
    Some(match &cs.to_be_bytes() {
        b"XYZ " | b"Lab " | b"Luv " | b"YCbr" | b"Yxy " | b"RGB " | b"HSV " | b"HLS " | b"CMY " => {
            3
        }
        b"GRAY" => 1,
        b"CMYK" => 4,
        [n @ b'2'..=b'9', b'C', b'L', b'R'] => (n - b'0') as u32,
        [n @ b'A'..=b'F', b'C', b'L', b'R'] => (n - b'A' + 10) as u32,
        _ => return None,
    })
}

fn is_xclr(cs: u32) -> bool {
    matches!(
        &cs.to_be_bytes(),
        [b'2'..=b'9' | b'A'..=b'F', b'C', b'L', b'R']
    )
}

/// Permitted type signatures for each tag signature: ICC.1:2022 9.2
/// (`v4 = true`) or ICC.1:2001-04 6.4.
pub fn permitted_types(tag: u32, v4: bool) -> Option<&'static [&'static [u8; 4]]> {
    let t = &tag.to_be_bytes();
    if v4 {
        Some(match t {
            b"A2B0" | b"A2B1" | b"A2B2" => &[b"mft1", b"mft2", b"mAB "],
            b"B2A0" | b"B2A1" | b"B2A2" | b"gamt" | b"pre1" | b"pre2" => {
                &[b"mft1", b"mft2", b"mBA "]
            }
            b"pre0" => &[b"mft1", b"mft2", b"mAB ", b"mBA "],
            b"B2D0" | b"B2D1" | b"B2D2" | b"B2D3" | b"D2B0" | b"D2B1" | b"D2B2" | b"D2B3" => {
                &[b"mpet"]
            }
            b"rXYZ" | b"gXYZ" | b"bXYZ" | b"lumi" | b"wtpt" => &[b"XYZ "],
            b"rTRC" | b"gTRC" | b"bTRC" | b"kTRC" => &[b"curv", b"para"],
            b"calt" => &[b"dtim"],
            b"targ" => &[b"text"],
            b"chad" => &[b"sf32"],
            b"chrm" => &[b"chrm"],
            b"cicp" => &[b"cicp"],
            b"clro" => &[b"clro"],
            b"clrt" | b"clot" => &[b"clrt"],
            b"ciis" | b"rig0" | b"rig2" | b"tech" => &[b"sig "],
            b"cprt" | b"desc" | b"dmnd" | b"dmdd" | b"vued" => &[b"mluc"],
            b"meas" => &[b"meas"],
            b"meta" => &[b"dict"],
            b"ncl2" => &[b"ncl2"],
            b"resp" => &[b"rcs2"],
            b"pseq" => &[b"pseq"],
            b"psid" => &[b"psid"],
            b"view" => &[b"view"],
            // Headroom Adaptive Gain Curve amendment, 1.1.
            b"HAGC" => &[b"hagc"],
            _ => return None,
        })
    } else {
        Some(match t {
            b"A2B0" | b"A2B1" | b"A2B2" | b"B2A0" | b"B2A1" | b"B2A2" | b"gamt" | b"pre0"
            | b"pre1" | b"pre2" => &[b"mft1", b"mft2"],
            b"rXYZ" | b"gXYZ" | b"bXYZ" | b"lumi" | b"wtpt" | b"bkpt" => &[b"XYZ "],
            b"rTRC" | b"gTRC" | b"bTRC" | b"kTRC" => &[b"curv"],
            b"calt" => &[b"dtim"],
            b"targ" | b"cprt" => &[b"text"],
            b"chad" => &[b"sf32"],
            b"chrm" => &[b"chrm"],
            b"crdi" => &[b"crdi"],
            b"desc" | b"dmnd" | b"dmdd" | b"scrd" | b"vued" => &[b"desc"],
            b"devs" => &[b"devs"],
            b"meas" => &[b"meas"],
            b"ncol" => &[b"ncol"],
            b"ncl2" => &[b"ncl2"],
            b"resp" => &[b"rcs2"],
            b"pseq" => &[b"pseq"],
            b"psd0" | b"psd1" | b"psd2" | b"psd3" | b"ps2s" | b"ps2i" => &[b"data"],
            b"scrn" => &[b"scrn"],
            b"tech" => &[b"sig "],
            b"bfd " => &[b"bfd "],
            b"view" => &[b"view"],
            _ => return None,
        })
    }
}

/// What the checker needs to know about the profile while checking a tag.
#[derive(Clone, Copy)]
struct Ctx {
    v4: bool,
    class: u32,
    cs: u32,
    pcs: u32,
}

struct Checker {
    out: Vec<Finding>,
}

impl Checker {
    fn shall(&mut self, clause: &'static str, tag: Option<u32>, msg: impl Into<String>) {
        self.out.push(Finding {
            level: Level::Shall,
            clause,
            tag: tag.map(sig_str),
            msg: msg.into(),
        });
    }
    fn should(&mut self, clause: &'static str, tag: Option<u32>, msg: impl Into<String>) {
        self.out.push(Finding {
            level: Level::Should,
            clause,
            tag: tag.map(sig_str),
            msg: msg.into(),
        });
    }
}

/// Check a whole profile. Returns every finding, sorted.
pub fn check(p: &[u8]) -> Vec<Finding> {
    let mut c = Checker { out: Vec::new() };
    if p.len() < 132 {
        c.shall(
            "ICC.1:2022 7.1.1",
            None,
            format!("{} bytes: no header and tag count", p.len()),
        );
        return c.out;
    }
    let size = be32(p, 0).unwrap() as usize;
    let v4 = p[8] >= 4;
    let class = be32(p, 12).unwrap();
    let cs = be32(p, 16).unwrap();
    let pcs = be32(p, 20).unwrap();
    let ctx = Ctx { v4, class, cs, pcs };

    // --- 7.2 header ------------------------------------------------------
    if size != p.len() {
        c.shall(
            "ICC.1:2022 7.2.2",
            None,
            format!("size field {size} != {} bytes", p.len()),
        );
    }
    if v4 && p.len() % 4 != 0 {
        c.shall(
            "ICC.1:2022 7.1.2 c",
            None,
            "profile length is not a multiple of four",
        );
    }
    let (maj, min, fix) = (p[8], p[9] >> 4, p[9] & 0xF);
    if !matches!(maj, 2 | 4) || min > 9 || fix > 9 || p[10] != 0 || p[11] != 0 {
        c.shall(
            "ICC.1:2022 7.2.4",
            None,
            format!("version bytes {:02x?} are not BCD x.y.z.0", &p[8..12]),
        );
    }
    if maj == 4 && (min, fix) > (4, 0) {
        c.shall(
            "ICC.1:2022 7.2.4",
            None,
            format!("version 4.{min}.{fix} is newer than 4.4.0.0"),
        );
    }
    if !matches!(
        &class.to_be_bytes(),
        b"scnr" | b"mntr" | b"prtr" | b"link" | b"spac" | b"abst" | b"nmcl"
    ) {
        c.shall(
            "ICC.1:2022 7.2.5",
            None,
            format!("class '{}' not in Table 18", sig_str(class)),
        );
    }
    if channels(cs).is_none() {
        c.shall(
            "ICC.1:2022 7.2.6",
            None,
            format!("data colour space '{}' not in Table 19", sig_str(cs)),
        );
    }
    let is_link = class == sig(b"link");
    if is_link {
        if channels(pcs).is_none() {
            c.shall(
                "ICC.1:2022 7.2.7",
                None,
                format!("link PCS field '{}' not in Table 19", sig_str(pcs)),
            );
        }
    } else if pcs != sig(b"XYZ ") && pcs != sig(b"Lab ") {
        c.shall(
            "ICC.1:2022 7.2.7",
            None,
            format!("PCS '{}' is neither XYZ nor Lab", sig_str(pcs)),
        );
    }
    check_date(&mut c, &p[24..36], None, "ICC.1:2022 7.2.8 / 4.2");
    if &p[36..40] != b"acsp" {
        c.shall("ICC.1:2022 7.2.9", None, "file signature is not 'acsp'");
    }
    let plat = be32(p, 40).unwrap();
    if plat != 0 && !matches!(&plat.to_be_bytes(), b"APPL" | b"MSFT" | b"SGI " | b"SUNW") {
        c.shall(
            "ICC.1:2022 7.2.10",
            None,
            format!("platform '{}' not in Table 20", sig_str(plat)),
        );
    }
    let flags = be32(p, 44).unwrap();
    if flags & 0xFFFC != 0 {
        c.shall(
            "ICC.1:2022 7.2.11",
            None,
            format!("ICC-reserved flag bits set: {flags:08x}"),
        );
    }
    let attr = u64::from_be_bytes(p[56..64].try_into().unwrap());
    if attr & 0xFFFF_FFF0 != 0 {
        c.shall(
            "ICC.1:2022 7.2.14",
            None,
            format!("reserved attribute bits 4..31 set: {attr:016x}"),
        );
    }
    let intent = be32(p, 64).unwrap();
    if intent > 3 {
        c.shall(
            "ICC.1:2022 7.2.15",
            None,
            format!("rendering intent field {intent:08x}"),
        );
    }
    let ill = [
        s15f16(p, 68).unwrap(),
        s15f16(p, 72).unwrap(),
        s15f16(p, 76).unwrap(),
    ];
    let r4 = |v: f64| (v * 1e4).round() / 1e4;
    if (r4(ill[0]), r4(ill[1]), r4(ill[2])) != (0.9642, 1.0, 0.8249) {
        c.shall(
            "ICC.1:2022 7.2.16",
            None,
            format!("PCS illuminant {ill:?} is not D50"),
        );
    }
    let id = &p[84..100];
    if !v4 {
        // ICC.1:2001-04 Table 13: bytes 84..127 are reserved; the profile
        // ID field arrived with version 4.
        if p[84..128].iter().any(|&b| b != 0) {
            c.should(
                "ICC.1:2001-04 6.1",
                None,
                "reserved header bytes 84..127 are not zero",
            );
        }
    } else if id.iter().any(|&b| b != 0) && size == p.len() {
        let mut q = p.to_vec();
        q[44..48].fill(0);
        q[64..68].fill(0);
        q[84..100].fill(0);
        use md5::{Digest, Md5};
        let want = Md5::digest(&q);
        if want.as_slice() != id {
            c.shall(
                "ICC.1:2022 7.2.18",
                None,
                "profile ID is not the MD5 of the profile",
            );
        }
    } else if v4 {
        c.should("ICC.1:2022 7.2.18", None, "no profile ID recorded");
    }
    if v4 && p[100..128].iter().any(|&b| b != 0) {
        c.shall(
            "ICC.1:2022 7.2.19",
            None,
            "reserved header bytes 100..127 are not zero",
        );
    }

    // --- 7.3 tag table ---------------------------------------------------
    let n = be32(p, 128).unwrap() as usize;
    let table_end = 132usize.saturating_add(n.saturating_mul(12));
    if table_end > p.len() {
        c.shall("ICC.1:2022 7.3.2", None, format!("{n} tags do not fit"));
        return finish(c);
    }
    let mut entries = Vec::with_capacity(n);
    let mut seen = BTreeSet::new();
    for i in 0..n {
        let o = 132 + 12 * i;
        let (t, off, sz) = (
            be32(p, o).unwrap(),
            be32(p, o + 4).unwrap() as usize,
            be32(p, o + 8).unwrap() as usize,
        );
        if !seen.insert(t) {
            c.shall("ICC.1:2022 7.3.1", Some(t), "duplicate tag signature");
        }
        if off % 4 != 0 {
            c.shall(
                "ICC.1:2022 7.3.4",
                Some(t),
                format!("offset {off} not on a 4-byte boundary"),
            );
        }
        if off.checked_add(sz).is_none_or(|e| e > p.len()) {
            c.shall(
                "ICC.1:2022 7.3.4",
                Some(t),
                format!("data {off}+{sz} runs past the profile"),
            );
            continue;
        }
        if off < table_end {
            c.shall(
                "ICC.1:2022 7.3.1",
                Some(t),
                "tag data overlaps the header or tag table",
            );
            continue;
        }
        entries.push((t, off, sz));
    }
    // Version 4 only (ICC.1:2001-04 6.2 asks for alignment alone): shared
    // data agrees in offset and size; different data does not overlap; data
    // is contiguous, starts right after the table and ends, padded with
    // NULs, at the end of the profile (7.1.2 b/c/d, 7.3.1, 7.4).
    let mut blocks: BTreeMap<usize, usize> = BTreeMap::new();
    for &(t, off, sz) in &entries {
        if let Some(&s) = blocks.get(&off) {
            if s != sz {
                c.shall(
                    "ICC.1:2022 7.3.1",
                    Some(t),
                    "shares an offset with a tag of different size",
                );
            }
        } else {
            blocks.insert(off, sz);
        }
    }
    let mut cursor = table_end;
    for (&off, &sz) in blocks.iter().filter(|_| v4) {
        if off < cursor {
            c.shall(
                "ICC.1:2022 7.3.1",
                None,
                format!("tag data at {off} overlaps the previous element"),
            );
        } else if off != align4(cursor) {
            c.shall(
                "ICC.1:2022 7.3.1",
                None,
                format!("gap before tag data at {off} (expected {})", align4(cursor)),
            );
        }
        let pad = &p[(off + sz).min(p.len())..align4(off + sz).min(p.len())];
        if pad.iter().any(|&b| b != 0) {
            c.shall(
                "ICC.1:2022 7.1.2 d",
                None,
                format!("non-zero pad bytes after tag data at {off}"),
            );
        }
        cursor = cursor.max(off + sz);
    }
    if v4 && !blocks.is_empty() && align4(cursor) != p.len() {
        c.shall(
            "ICC.1:2022 7.1.2 c",
            None,
            format!(
                "profile ends at {} but tag data ends at {}",
                p.len(),
                align4(cursor)
            ),
        );
    }
    if let Some((&first, _)) = blocks.iter().next().filter(|_| v4) {
        if first != table_end {
            c.shall(
                "ICC.1:2022 7.1.2 b",
                None,
                format!("first tag data at {first}, tag table ends at {table_end}"),
            );
        }
    }

    // --- 8 / v2 6.3 required tags ---------------------------------------
    let has = |s: &[u8; 4]| seen.contains(&sig(s));
    check_required(&mut c, ctx, &has);

    // --- 9 / 10 (v2 6.4 / 6.5) each tag ----------------------------------
    for &(t, off, sz) in &entries {
        let d = &p[off..off + sz];
        check_tag(&mut c, ctx, t, d);
    }
    finish(c)
}

fn finish(mut c: Checker) -> Vec<Finding> {
    c.out.sort();
    c.out.dedup();
    c.out
}

fn check_required(c: &mut Checker, ctx: Ctx, has: &dyn Fn(&[u8; 4]) -> bool) {
    let cl = &ctx.class.to_be_bytes();
    let (base, sec) = if ctx.v4 {
        ("ICC.1:2022 8.2", "ICC.1:2022 8")
    } else {
        ("ICC.1:2001-04 6.3", "ICC.1:2001-04 6.3")
    };
    let need = |c: &mut Checker, s: &[u8; 4], clause: &'static str| {
        if !has(s) {
            c.shall(
                clause,
                None,
                format!("required tag '{}' missing", String::from_utf8_lossy(s)),
            );
        }
    };
    need(c, b"desc", base);
    need(c, b"cprt", base);
    if cl != b"link" {
        need(c, b"wtpt", base);
    }
    let matrix = [b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC"];
    match cl {
        b"scnr" | b"mntr" => {
            // One of the three models: N-component LUT, matrix/TRC, gray.
            let lut = has(b"A2B0") && (cl == b"scnr" || has(b"B2A0"));
            let mat = matrix.iter().all(|s| has(s));
            let gray = has(b"kTRC");
            if !(lut || mat || gray) {
                c.shall(sec, None, "no complete LUT, matrix/TRC or grayTRC model");
            }
            if mat && !lut && ctx.pcs != sig(b"XYZ ") {
                c.shall(
                    "ICC.1:2022 8.3.3",
                    None,
                    "matrix/TRC model with a PCSLAB header",
                );
            }
        }
        b"prtr" => {
            if !has(b"kTRC") {
                for s in [
                    b"A2B0", b"A2B1", b"A2B2", b"B2A0", b"B2A1", b"B2A2", b"gamt",
                ] {
                    need(c, s, "ICC.1:2022 8.5.2");
                }
                if is_xclr(ctx.cs) {
                    need(c, b"clrt", "ICC.1:2022 8.5.2");
                }
            }
        }
        b"link" => {
            need(
                c,
                b"pseq",
                if ctx.v4 {
                    "ICC.1:2022 8.6"
                } else {
                    "ICC.1:2001-04 6.3.4.1"
                },
            );
            need(
                c,
                b"A2B0",
                if ctx.v4 {
                    "ICC.1:2022 8.6"
                } else {
                    "ICC.1:2001-04 6.3.4.1"
                },
            );
            if ctx.v4 && is_xclr(ctx.cs) {
                need(c, b"clrt", "ICC.1:2022 8.6");
            }
            if ctx.v4 && is_xclr(ctx.pcs) {
                need(c, b"clot", "ICC.1:2022 8.6");
            }
        }
        b"spac" => {
            need(c, b"A2B0", "ICC.1:2022 8.7");
            need(c, b"B2A0", "ICC.1:2022 8.7");
        }
        b"abst" => need(c, b"A2B0", "ICC.1:2022 8.8"),
        b"nmcl" => need(c, b"ncl2", "ICC.1:2022 8.9"),
        _ => {}
    }
}

fn check_date(c: &mut Checker, d: &[u8], tag: Option<u32>, clause: &'static str) {
    let f = |i: usize| be16(d, 2 * i).unwrap_or(0);
    let (y, mo, da, h, mi, s) = (f(0), f(1), f(2), f(3), f(4), f(5));
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || s > 59 || y == 0 {
        c.shall(
            clause,
            tag,
            format!("dateTimeNumber {y}-{mo}-{da} {h}:{mi}:{s} out of range"),
        );
    }
}

fn check_tag(c: &mut Checker, ctx: Ctx, t: u32, d: &[u8]) {
    let clause_types = if ctx.v4 {
        "ICC.1:2022 9.2"
    } else {
        "ICC.1:2001-04 6.4"
    };
    if d.len() < 8 {
        c.shall(
            "ICC.1:2022 10.1",
            Some(t),
            "shorter than a type signature and reserved field",
        );
        return;
    }
    let ty = be32(d, 0).unwrap();
    match permitted_types(t, ctx.v4) {
        Some(ok) => {
            if !ok.iter().any(|s| sig(s) == ty) {
                c.shall(
                    clause_types,
                    Some(t),
                    format!(
                        "type '{}' not permitted (allowed: {})",
                        sig_str(ty),
                        ok.iter()
                            .map(|s| String::from_utf8_lossy(*s).into_owned())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
        None => {
            // Private or other-version tag: only registered tags should
            // appear (7.3.1). The ICC tag registry is not archived, so
            // known registered private tags are listed here.
            if !matches!(&t.to_be_bytes(), b"vcgt" | b"MHC2") {
                c.should(
                    "ICC.1:2022 7.3.1",
                    Some(t),
                    "tag not defined for this profile version",
                );
            }
        }
    }
    if be32(d, 4) != Some(0) {
        c.shall(
            "ICC.1:2022 10.1",
            Some(t),
            "reserved bytes 4..7 are not zero",
        );
    }
    // Tag-specific placement rules.
    match &t.to_be_bytes() {
        b"cicp" if ctx.v4 => {
            let cls_ok = matches!(&ctx.class.to_be_bytes(), b"scnr" | b"mntr");
            let cs_ok = matches!(&ctx.cs.to_be_bytes(), b"RGB " | b"YCbr" | b"XYZ ");
            if !cls_ok || !cs_ok {
                c.shall(
                    "ICC.1:2022 9.2.17",
                    Some(t),
                    "cicp tag outside Input/Display RGB/YCbCr/XYZ profiles",
                );
            }
        }
        b"clot" if ctx.v4 && ctx.class != sig(b"link") => {
            c.shall(
                "ICC.1:2022 9.2.20",
                Some(t),
                "colorantTableOutTag outside a DeviceLink profile",
            );
        }
        b"wtpt" if ctx.v4 && ctx.class == sig(b"mntr") => {
            if let (Some(x), Some(y), Some(z)) = (s15f16(d, 8), s15f16(d, 12), s15f16(d, 16)) {
                let r4 = |v: f64| (v * 1e4).round() / 1e4;
                if (r4(x), r4(y), r4(z)) != (0.9642, 1.0, 0.8249) {
                    c.shall("ICC.1:2022 9.2.36", Some(t), format!("display media white point {x:.4} {y:.4} {z:.4} is not the PCS illuminant"));
                }
            }
        }
        b"chad" => {
            if d.len() != 8 + 36 {
                c.shall(
                    "ICC.1:2022 9.2.15",
                    Some(t),
                    format!("{} bytes, not a 3x3 matrix", d.len()),
                );
            } else {
                let m: Vec<f64> = (0..9).map(|i| s15f16(d, 8 + 4 * i).unwrap()).collect();
                let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
                    + m[2] * (m[3] * m[7] - m[4] * m[6]);
                if det.abs() < 1e-9 {
                    c.shall(
                        "ICC.1:2022 9.2.15",
                        Some(t),
                        "chromatic adaptation matrix is not invertible",
                    );
                }
            }
        }
        _ => {}
    }
    check_type(c, ctx, t, ty, d);
}

/// Check one tag data element against its type definition (clause 10 /
/// v2 6.5).
fn check_type(c: &mut Checker, ctx: Ctx, t: u32, ty: u32, d: &[u8]) {
    let tg = Some(t);
    let len = d.len();
    let multiple = |c: &mut Checker, unit: usize, clause: &'static str| {
        if len < 8 || (len - 8) % unit != 0 {
            c.shall(clause, tg, format!("{len} bytes is not 8 + n*{unit}"));
        }
    };
    match &ty.to_be_bytes() {
        b"XYZ " => {
            multiple(c, 12, "ICC.1:2022 10.31");
            if len < 20 {
                c.shall("ICC.1:2022 10.31", tg, "no XYZNumber");
            }
        }
        b"curv" => {
            check_curv(c, tg, d, true);
        }
        b"para" => {
            check_para(c, tg, d, true);
        }
        b"sf32" => multiple(c, 4, "ICC.1:2022 10.22"),
        b"uf32" => multiple(c, 4, "ICC.1:2022 10.25"),
        b"ui08" => multiple(c, 1, "ICC.1:2022 10.29"),
        b"ui16" => multiple(c, 2, "ICC.1:2022 10.26"),
        b"ui32" => multiple(c, 4, "ICC.1:2022 10.27"),
        b"ui64" => multiple(c, 8, "ICC.1:2022 10.28"),
        b"sig " => {
            if len != 12 {
                c.shall("ICC.1:2022 10.23", tg, format!("{len} bytes, expected 12"));
            }
        }
        b"dtim" => {
            if len != 20 {
                c.shall("ICC.1:2022 10.8", tg, format!("{len} bytes, expected 20"));
            } else {
                check_date(c, &d[8..20], tg, "ICC.1:2022 10.8 / 4.2");
            }
        }
        b"text" => {
            if len < 9 || d[len - 1] != 0 {
                c.shall("ICC.1:2022 10.24", tg, "text is not NUL-terminated");
            }
            if d[8..].iter().any(|&b| b >= 0x80) {
                c.shall("ICC.1:2022 10.24", tg, "text is not 7-bit ASCII");
            }
        }
        b"data" => match be32(d, 8) {
            Some(0) => {
                if len < 13 || d[len - 1] != 0 {
                    c.shall("ICC.1:2022 10.7", tg, "ASCII data is not NUL-terminated");
                }
            }
            Some(1) => {}
            Some(f) => c.shall("ICC.1:2022 10.7", tg, format!("data flag {f} is reserved")),
            None => c.shall("ICC.1:2022 10.7", tg, "no data flag"),
        },
        b"mluc" => {
            let end = check_mluc(c, tg, d);
            check_tag_end(c, ctx, tg, d, end);
        }
        b"desc" => {
            let end = check_text_description(c, tg, d);
            check_tag_end(c, ctx, tg, d, end);
        }
        b"chrm" => check_chrm(c, tg, d),
        b"cicp" => check_cicp(c, ctx, tg, d),
        b"clro" => {
            let n = be32(d, 8).unwrap_or(0) as usize;
            if len != 12 + n {
                c.shall(
                    "ICC.1:2022 10.4",
                    tg,
                    format!("{len} bytes for {n} colorants"),
                );
            }
            if channels(ctx.cs) != Some(n as u32) {
                c.shall(
                    "ICC.1:2022 10.4",
                    tg,
                    format!(
                        "{n} colorants, data colour space has {:?}",
                        channels(ctx.cs)
                    ),
                );
            }
        }
        b"clrt" => {
            let n = be32(d, 8).unwrap_or(0) as usize;
            if len != 12 + 38 * n {
                c.shall(
                    "ICC.1:2022 10.5",
                    tg,
                    format!("{len} bytes for {n} colorants"),
                );
            } else {
                for k in 0..n {
                    check_name32(
                        c,
                        tg,
                        &d[12 + 38 * k..12 + 38 * k + 32],
                        "ICC.1:2022 10.5",
                        true,
                    );
                }
            }
            let space = if t == sig(b"clot") { ctx.pcs } else { ctx.cs };
            if channels(space) != Some(n as u32) {
                c.shall(
                    "ICC.1:2022 10.5",
                    tg,
                    format!(
                        "{n} colorants, colour space '{}' has {:?}",
                        sig_str(space),
                        channels(space)
                    ),
                );
            }
        }
        b"meas" => {
            if len != 36 {
                c.shall("ICC.1:2022 10.14", tg, format!("{len} bytes, expected 36"));
            } else {
                if be32(d, 8).unwrap() > 2 {
                    c.shall("ICC.1:2022 10.14", tg, "standard observer not in Table 50");
                }
                if be32(d, 24).unwrap() > 2 {
                    c.shall("ICC.1:2022 10.14", tg, "geometry not in Table 51");
                }
                if be32(d, 32).unwrap() > 8 {
                    c.shall("ICC.1:2022 10.14", tg, "illuminant not in Table 53");
                }
            }
        }
        b"view" => {
            if len != 36 {
                c.shall("ICC.1:2022 10.30", tg, format!("{len} bytes, expected 36"));
            } else if be32(d, 32).unwrap() > 8 {
                c.shall("ICC.1:2022 10.30", tg, "illuminant type not in Table 53");
            }
        }
        b"mft1" | b"mft2" => check_lut(c, ctx, t, tg, d, ty == sig(b"mft2")),
        b"mAB " | b"mBA " => check_mab(c, ctx, tg, d, ty == sig(b"mAB ")),
        b"mpet" => check_mpet(c, tg, d),
        b"ncl2" => check_ncl2(c, ctx, tg, d),
        b"pseq" => check_pseq(c, tg, d),
        b"psid" => check_psid(c, tg, d),
        b"dict" => check_dict(c, tg, d),
        b"rcs2" => check_rcs2(c, tg, d),
        b"scrn" => {
            let n = be32(d, 12).unwrap_or(0) as usize;
            if len != 16 + 12 * n {
                c.shall(
                    "ICC.1:2001-04 6.5.15",
                    tg,
                    format!("{len} bytes for {n} channels"),
                );
            }
        }
        b"bfd " => check_ucrbg(c, tg, d),
        b"crdi" => check_crdi(c, tg, d),
        _ => {}
    }
}

/// The tag data element size is "the number of actual data bytes and shall
/// not include any padding at the end" (ICC.1:2022 7.3.5; ICC.1:2001-04
/// 6.2.3: "must not include any padding").
fn check_tag_end(c: &mut Checker, ctx: Ctx, tg: Option<u32>, d: &[u8], end: usize) {
    if end >= d.len() {
        return;
    }
    let clause = if ctx.v4 {
        "ICC.1:2022 7.3.5"
    } else {
        "ICC.1:2001-04 6.2.3"
    };
    if align4(end) == d.len() && d[end..].iter().all(|&b| b == 0) {
        c.shall(clause, tg, "tag size includes trailing pad bytes");
    } else {
        c.should(
            clause,
            tg,
            format!("{} bytes after the end of the structure", d.len() - end),
        );
    }
}

/// curveType (10.6). Returns the element length.
fn check_curv(c: &mut Checker, tg: Option<u32>, d: &[u8], whole: bool) -> usize {
    let Some(n) = be32(d, 8) else {
        c.shall("ICC.1:2022 10.6", tg, "curveType without a count");
        return d.len();
    };
    let need = 12 + 2 * n as usize;
    if (whole && d.len() != need) || d.len() < need {
        c.shall(
            "ICC.1:2022 10.6",
            tg,
            format!("{} bytes for {n} entries", d.len()),
        );
    }
    need
}

/// parametricCurveType (10.18, Table 68). Returns the element length.
fn check_para(c: &mut Checker, tg: Option<u32>, d: &[u8], whole: bool) -> usize {
    let Some(ft) = be16(d, 8) else {
        c.shall(
            "ICC.1:2022 10.18",
            tg,
            "parametricCurveType without a function type",
        );
        return d.len();
    };
    if be16(d, 10) != Some(0) {
        c.shall("ICC.1:2022 10.18", tg, "reserved bytes 10..11 are not zero");
    }
    let params = match ft {
        0 => 1,
        1 => 3,
        2 => 4,
        3 => 5,
        4 => 7,
        _ => {
            c.shall(
                "ICC.1:2022 10.18",
                tg,
                format!("function type {ft} not in Table 68"),
            );
            return d.len();
        }
    };
    let need = 12 + 4 * params;
    if (whole && d.len() != need) || d.len() < need {
        c.shall(
            "ICC.1:2022 10.18",
            tg,
            format!("{} bytes for function type {ft}", d.len()),
        );
    }
    need
}

fn check_embedded_curve(c: &mut Checker, tg: Option<u32>, d: &[u8], v4: bool) -> usize {
    match be32(d, 0).map(|v| v.to_be_bytes()) {
        Some(b) if &b == b"curv" => check_curv(c, tg, d, false),
        Some(b) if &b == b"para" && v4 => check_para(c, tg, d, false),
        other => {
            c.shall(
                "ICC.1:2022 10.12.2",
                tg,
                format!(
                    "embedded curve type {:?} is not curv/para",
                    other.map(|b| String::from_utf8_lossy(&b).into_owned())
                ),
            );
            d.len()
        }
    }
}

/// multiLocalizedUnicodeType (10.15). Returns the element length.
fn check_mluc(c: &mut Checker, tg: Option<u32>, d: &[u8]) -> usize {
    let cl = "ICC.1:2022 10.15";
    let (Some(n), Some(rs)) = (be32(d, 8), be32(d, 12)) else {
        c.shall(cl, tg, "mluc header truncated");
        return d.len();
    };
    if rs != 12 {
        c.shall(cl, tg, format!("record size {rs}, expected 12"));
        return d.len();
    }
    let rec_end = 16 + 12 * n as usize;
    if rec_end > d.len() {
        c.shall(
            cl,
            tg,
            format!("{n} records do not fit in {} bytes", d.len()),
        );
        return d.len();
    }
    let mut end = rec_end;
    for i in 0..n as usize {
        let r = 16 + 12 * i;
        let (lang, ctry) = (&d[r..r + 2], &d[r + 2..r + 4]);
        let (slen, soff) = (
            be32(d, r + 4).unwrap() as usize,
            be32(d, r + 8).unwrap() as usize,
        );
        // Table 54: ISO 639-1 / ISO 3166-1 codes; 10.1 makes 0 "unknown".
        if lang != [0, 0] && !lang.iter().all(|b| b.is_ascii_alphabetic()) {
            c.shall(
                cl,
                tg,
                format!("language code {lang:02x?} is not an ISO 639-1 code"),
            );
        }
        if ctry != [0, 0] && !ctry.iter().all(|b| b.is_ascii_alphabetic()) {
            c.shall(
                cl,
                tg,
                format!("country code {ctry:02x?} is not an ISO 3166-1 code"),
            );
        }
        if soff < rec_end || soff + slen > d.len() {
            c.shall(
                cl,
                tg,
                format!("string {soff}+{slen} outside the storage area"),
            );
            continue;
        }
        if slen % 2 != 0 {
            c.shall(
                cl,
                tg,
                format!("string length {slen} is not whole UTF-16 units"),
            );
        }
        if slen >= 2 && d[soff + slen - 2] == 0 && d[soff + slen - 1] == 0 {
            c.should(cl, tg, "string is NUL-terminated");
        }
        end = end.max(soff + slen);
    }
    end
}

/// textDescriptionType (v2 6.5.17). Returns the element length.
fn check_text_description(c: &mut Checker, tg: Option<u32>, d: &[u8]) -> usize {
    let cl = "ICC.1:2001-04 6.5.17";
    let Some(a) = be32(d, 8) else {
        c.shall(cl, tg, "truncated");
        return d.len();
    };
    let a = a as usize;
    let m = 12 + a;
    if a == 0 || m > d.len() || d[m - 1] != 0 {
        c.shall(cl, tg, "ASCII description missing or not NUL-terminated");
        return d.len();
    }
    if d[12..m].iter().any(|&b| b >= 0x80) {
        c.shall(cl, tg, "ASCII description is not 7-bit");
    }
    let (Some(_lang), Some(u)) = (be32(d, m), be32(d, m + 4)) else {
        c.shall(cl, tg, "Unicode part truncated");
        return d.len();
    };
    let sc = m + 8 + 2 * u as usize;
    let (Some(_code), Some(&mc)) = (be16(d, sc), d.get(sc + 2)) else {
        c.shall(cl, tg, "ScriptCode part truncated");
        return d.len();
    };
    if mc > 67 {
        c.shall(cl, tg, format!("ScriptCode count {mc} > 67"));
    }
    let end = sc + 3 + 67;
    if end > d.len() {
        c.shall(cl, tg, format!("{} bytes, structure needs {end}", d.len()));
    }
    end
}

fn check_chrm(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.2";
    let (Some(n), Some(ty)) = (be16(d, 8), be16(d, 10)) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    if d.len() != 12 + 8 * n as usize {
        c.shall(cl, tg, format!("{} bytes for {n} channels", d.len()));
        return;
    }
    let table: [[f64; 6]; 6] = [
        [0.640, 0.330, 0.300, 0.600, 0.150, 0.060],
        [0.630, 0.340, 0.310, 0.595, 0.155, 0.070],
        [0.640, 0.330, 0.290, 0.600, 0.150, 0.060],
        [0.625, 0.340, 0.280, 0.605, 0.155, 0.070],
        [0.680, 0.320, 0.265, 0.690, 0.150, 0.060],
        [0.780, 0.292, 0.170, 0.797, 0.131, 0.046],
    ];
    match ty {
        0 => {}
        1..=6 => {
            if ty <= 4 && n != 3 {
                c.shall(
                    cl,
                    tg,
                    "colorant type 1..4 with a channel count other than 3",
                );
            }
            if n == 3 {
                let row = table[ty as usize - 1];
                for (i, want) in row.iter().enumerate() {
                    let got = be32(d, 12 + 4 * i).unwrap() as f64 / 65536.0;
                    if (got - want).abs() > 1.5 / 65536.0 {
                        c.shall(
                            cl,
                            tg,
                            format!(
                                "colorant type {ty}: value {i} is {got:.4}, Table 31 says {want}"
                            ),
                        );
                    }
                }
            }
        }
        _ => c.shall(cl, tg, format!("colorant type {ty} not in Table 31")),
    }
}

fn check_cicp(c: &mut Checker, ctx: Ctx, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.3";
    if d.len() < 12 {
        c.shall(cl, tg, format!("{} bytes, expected 12", d.len()));
        return;
    }
    let (cp, mc, fr) = (d[8], d[10], d[11]);
    match d.len() {
        12 => {}
        44 if cp == 2 => {}
        44 => c.shall(
            "ICC.1 cicp amendment 2026-10-19",
            tg,
            "chromaticity extension with ColourPrimaries != 2",
        ),
        n => c.shall(
            cl,
            tg,
            format!("{n} bytes, expected 12 (or 44 with the 2026 amendment)"),
        ),
    }
    let cs = &ctx.cs.to_be_bytes();
    if (cs == b"RGB " || cs == b"XYZ ") && mc != 0 {
        c.shall(
            cl,
            tg,
            format!("MatrixCoefficients {mc} with an RGB/XYZ data colour space"),
        );
    }
    if cs == b"YCbr" && mc == 0 {
        c.shall(
            cl,
            tg,
            "MatrixCoefficients 0 with a YCbCr data colour space",
        );
    }
    if fr > 1 {
        c.shall(
            "ITU-T H.273 8.3",
            tg,
            format!("VideoFullRangeFlag {fr} is not 0 or 1"),
        );
    }
}

/// A 32-byte NUL-terminated 7-bit ASCII name field.
fn check_name32(c: &mut Checker, tg: Option<u32>, f: &[u8], cl: &'static str, unused_zero: bool) {
    match f.iter().position(|&b| b == 0) {
        None => c.shall(cl, tg, "32-byte name is not NUL-terminated"),
        Some(z) => {
            if f[..z].iter().any(|&b| b >= 0x80) {
                c.shall(cl, tg, "name is not 7-bit ASCII");
            }
            if unused_zero && f[z..].iter().any(|&b| b != 0) {
                c.shall(cl, tg, "unused name bytes are not zero");
            }
        }
    }
}

/// lut8Type / lut16Type (10.10, 10.11; v2 6.5.7, 6.5.8).
fn check_lut(c: &mut Checker, ctx: Ctx, t: u32, tg: Option<u32>, d: &[u8], is16: bool) {
    let cl = if is16 {
        "ICC.1:2022 10.10"
    } else {
        "ICC.1:2022 10.11"
    };
    if d.len() < if is16 { 52 } else { 48 } {
        c.shall(cl, tg, "truncated");
        return;
    }
    let (i, o, g) = (d[8] as usize, d[9] as usize, d[10] as usize);
    if d[11] != 0 {
        c.shall(cl, tg, "padding byte 11 is not zero");
    }
    let (n, m) = if is16 {
        (be16(d, 48).unwrap() as usize, be16(d, 50).unwrap() as usize)
    } else {
        (256, 256)
    };
    if is16 && (!(2..=4096).contains(&n) || !(2..=4096).contains(&m)) {
        c.shall(
            cl,
            tg,
            format!("table entries in={n} out={m}, allowed 2..4096"),
        );
    }
    // The lut8/lut16 model always applies its CLUT, sampled at increments of
    // 1/(K-1); fewer than two grid points leaves nothing to evaluate.
    if g < 2 {
        c.shall(cl, tg, format!("CLUT with {g} grid points"));
    }
    let clut = g
        .checked_pow(i as u32)
        .unwrap_or(usize::MAX)
        .saturating_mul(o);
    let want = if is16 {
        52 + 2 * (n * i + clut + m * o)
    } else {
        48 + 256 * i + clut + 256 * o
    };
    if d.len() != want {
        c.shall(cl, tg, format!("{} bytes, layout gives {want}", d.len()));
    }
    // The matrix shall be the identity unless the input is PCSXYZ.
    // A2Bx tags take the data colour space (a PCS encoding for Abstract
    // profiles); every other LUT tag takes the PCS.
    let input = if t.to_be_bytes().starts_with(b"A2B") {
        ctx.cs
    } else {
        ctx.pcs
    };
    if input != sig(b"XYZ ") {
        let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        for (k, want) in id.iter().enumerate() {
            if s15f16(d, 12 + 4 * k) != Some(*want) {
                c.shall(
                    cl,
                    tg,
                    "matrix is not the identity and the input is not PCSXYZ",
                );
                break;
            }
        }
    }
}

/// lutAToBType / lutBToAType (10.12, 10.13).
fn check_mab(c: &mut Checker, ctx: Ctx, tg: Option<u32>, d: &[u8], atob: bool) {
    let cl = if atob {
        "ICC.1:2022 10.12"
    } else {
        "ICC.1:2022 10.13"
    };
    if d.len() < 32 {
        c.shall(cl, tg, "truncated");
        return;
    }
    let (i, o) = (d[8] as usize, d[9] as usize);
    if d[10] != 0 || d[11] != 0 {
        c.shall(cl, tg, "padding bytes 10..11 are not zero");
    }
    let off = |k: usize| be32(d, 12 + 4 * k).unwrap() as usize;
    let (ob, omat, om, oclut, oa) = (off(0), off(1), off(2), off(3), off(4));
    for (name, v) in [
        ("B", ob),
        ("matrix", omat),
        ("M", om),
        ("CLUT", oclut),
        ("A", oa),
    ] {
        if v != 0 && (v % 4 != 0 || v < 32 || v >= d.len()) {
            c.shall(
                cl,
                tg,
                format!("{name} offset {v} is misaligned or outside the tag"),
            );
            return;
        }
    }
    let present = (ob != 0, omat != 0, om != 0, oclut != 0, oa != 0);
    let allowed = if atob {
        matches!(
            present,
            (true, false, false, false, false)
                | (true, true, true, false, false)
                | (true, false, false, true, true)
                | (true, true, true, true, true)
        )
    } else {
        // B; B,Matrix,M; B,CLUT,A; B,Matrix,M,CLUT,A.
        matches!(
            present,
            (true, false, false, false, false)
                | (true, true, true, false, false)
                | (true, false, false, true, true)
                | (true, true, true, true, true)
        )
    };
    if !atob && ctx.pcs != sig(b"XYZ ") && ctx.pcs != sig(b"Lab ") {
        c.shall(
            "ICC.1:2022 10.13.1",
            tg,
            "lutBToAType with a PCS field that is not PCSXYZ or PCSLAB",
        );
    }
    // Curve counts: mAB  A=i, M=o, B=o; mBA  B=i, M=i, A=o. The matrix is
    // 3x3 plus offsets, so it needs three channels on both sides.
    let (na, nm, nb) = if atob { (i, o, o) } else { (o, i, i) };
    let mut broken = false;
    if !allowed {
        c.shall(
            cl,
            tg,
            format!("element combination B/Mat/M/CLUT/A = {present:?} not permitted"),
        );
        broken = true;
    }
    if i != o && oclut == 0 {
        c.shall(cl, tg, format!("{i} in / {o} out channels without a CLUT"));
        broken = true;
    }
    if omat != 0 && nm != 3 {
        c.shall(cl, tg, format!("matrix with {nm} M curves (needs 3)"));
        broken = true;
    }
    if broken {
        // The element offsets cannot be trusted to follow the layout.
        return;
    }
    let curves = |c: &mut Checker, start: usize, count: usize| {
        let mut p = start;
        for _ in 0..count {
            if p >= d.len() {
                c.shall(cl, tg, "curve set runs past the tag");
                return;
            }
            let l = check_embedded_curve(c, tg, &d[p..], ctx.v4);
            let next = align4(p + l);
            if d[(p + l).min(d.len())..next.min(d.len())]
                .iter()
                .any(|&b| b != 0)
            {
                c.shall(cl, tg, "non-zero padding between curves");
            }
            p = next;
        }
    };
    if ob != 0 {
        curves(c, ob, nb);
    }
    if om != 0 {
        curves(c, om, nm);
    }
    if oa != 0 {
        curves(c, oa, na);
    }
    if oclut != 0 {
        let gp = &d[oclut..(oclut + 16).min(d.len())];
        if gp.len() < 16 || d.len() < oclut + 20 {
            c.shall(cl, tg, "CLUT header truncated");
            return;
        }
        if gp[i.min(16)..].iter().any(|&b| b != 0) {
            c.shall(cl, tg, "unused CLUT grid entries are not zero");
        }
        let prec = d[oclut + 16] as usize;
        if prec != 1 && prec != 2 {
            c.shall(cl, tg, format!("CLUT precision {prec}"));
        }
        if d[oclut + 17..oclut + 20].iter().any(|&b| b != 0) {
            c.shall(cl, tg, "CLUT padding bytes 17..19 are not zero");
        }
        let pts: usize = gp[..i.min(16)].iter().map(|&g| g as usize).product();
        if oclut + 20 + pts * o * prec > d.len() {
            c.shall(cl, tg, "CLUT data runs past the tag");
        }
    }
}

fn finite_normal(v: f32) -> bool {
    v == 0.0 || v.is_normal()
}

/// multiProcessElementsType (10.16).
fn check_mpet(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.16";
    let (Some(pin), Some(pout), Some(n)) = (be16(d, 8), be16(d, 10), be32(d, 12)) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    if n == 0 {
        c.shall(cl, tg, "no processing elements");
        return;
    }
    let n = n as usize;
    let table_end = 16 + 8 * n;
    if table_end > d.len() {
        c.shall(cl, tg, "position table runs past the tag");
        return;
    }
    let mut chan = pin;
    let mut min_off = usize::MAX;
    for k in 0..n {
        let (eo, es) = (
            be32(d, 16 + 8 * k).unwrap() as usize,
            be32(d, 20 + 8 * k).unwrap() as usize,
        );
        min_off = min_off.min(eo);
        if eo % 4 != 0 || eo < table_end || eo + es > d.len() || es < 12 {
            c.shall(
                cl,
                tg,
                format!("element {k} at {eo}+{es} misaligned or outside the tag"),
            );
            return;
        }
        let e = &d[eo..eo + es];
        let (ety, ep, eq) = (
            be32(e, 0).unwrap(),
            be16(e, 8).unwrap(),
            be16(e, 10).unwrap(),
        );
        if be32(e, 4) != Some(0) {
            c.shall(
                "ICC.1:2022 10.16.2.1",
                tg,
                format!("element {k} reserved bytes are not zero"),
            );
        }
        if ep != chan {
            c.shall(
                "ICC.1:2022 10.16.2.1",
                tg,
                format!("element {k} has {ep} inputs, previous output {chan}"),
            );
        }
        chan = eq;
        match &ety.to_be_bytes() {
            b"cvst" => check_cvst(c, tg, e, ep, eq),
            b"matf" => {
                let want = 12 + 4 * eq as usize * (ep as usize + 1);
                if e.len() != want {
                    c.shall(
                        "ICC.1:2022 10.16.2.3",
                        tg,
                        format!("matrix element {} bytes, expected {want}", e.len()),
                    );
                } else if (0..(want - 12) / 4)
                    .any(|j| !finite_normal(f32be(e, 12 + 4 * j).unwrap()))
                {
                    c.shall(
                        "ICC.1:2022 4.3",
                        tg,
                        "matrix element holds NaN, infinity or a denormal",
                    );
                }
            }
            b"clut" => {
                if e.len() < 28 {
                    c.shall("ICC.1:2022 10.16.2.4", tg, "CLUT element truncated");
                } else {
                    let g = &e[12..28];
                    if g[..(ep as usize).min(16)].iter().any(|&x| x < 2) {
                        c.shall(
                            "ICC.1:2022 10.16.2.4",
                            tg,
                            "CLUT dimension with fewer than two grid points",
                        );
                    }
                    if g[(ep as usize).min(16)..].iter().any(|&x| x != 0) {
                        c.shall(
                            "ICC.1:2022 10.16.2.4",
                            tg,
                            "unused CLUT grid entries are not zero",
                        );
                    }
                    let pts: usize = g[..(ep as usize).min(16)]
                        .iter()
                        .map(|&x| x as usize)
                        .product();
                    let want = 28 + 4 * pts * eq as usize;
                    if e.len() != want {
                        c.shall(
                            "ICC.1:2022 10.16.2.4",
                            tg,
                            format!("CLUT element {} bytes, expected {want}", e.len()),
                        );
                    } else if (0..pts * eq as usize)
                        .any(|j| !finite_normal(f32be(e, 28 + 4 * j).unwrap()))
                    {
                        c.shall(
                            "ICC.1:2022 4.3",
                            tg,
                            "CLUT element holds NaN, infinity or a denormal",
                        );
                    }
                }
            }
            b"bACS" | b"eACS" => {
                if ep != eq {
                    c.shall(
                        "ICC.1:2022 10.16.2.5",
                        tg,
                        "bACS/eACS with different input and output counts",
                    );
                }
            }
            _ => c.should(
                "ICC.1:2022 10.16.1",
                tg,
                format!("undefined processing element '{}'", sig_str(ety)),
            ),
        }
    }
    if min_off != table_end {
        c.shall(
            cl,
            tg,
            format!("first element at {min_off}, expected {table_end}"),
        );
    }
    if chan != pout {
        c.shall(
            "ICC.1:2022 10.16.2.1",
            tg,
            format!("last element outputs {chan}, tag declares {pout}"),
        );
    }
}

fn check_cvst(c: &mut Checker, tg: Option<u32>, e: &[u8], p: u16, q: u16) {
    let cl = "ICC.1:2022 10.16.2.2";
    if p != q {
        c.shall(cl, tg, "curve set with different input and output counts");
    }
    let p = p as usize;
    if e.len() < 12 + 8 * p {
        c.shall(cl, tg, "curve positions truncated");
        return;
    }
    let mut min_off = usize::MAX;
    for k in 0..p {
        let (o, s) = (
            be32(e, 12 + 8 * k).unwrap() as usize,
            be32(e, 16 + 8 * k).unwrap() as usize,
        );
        min_off = min_off.min(o);
        if o % 4 != 0 || o + s > e.len() {
            c.shall(
                cl,
                tg,
                format!("curve {k} at {o}+{s} misaligned or outside the element"),
            );
            continue;
        }
        check_curf(c, tg, &e[o..o + s]);
    }
    if p > 0 && min_off != 12 + 8 * p {
        c.shall(
            cl,
            tg,
            format!("first curve at {min_off}, expected {}", 12 + 8 * p),
        );
    }
}

fn check_curf(c: &mut Checker, tg: Option<u32>, s: &[u8]) {
    let cl = "ICC.1:2022 10.16.2.2";
    if be32(s, 0) != Some(sig(b"curf")) || s.len() < 12 {
        c.shall(cl, tg, "curve is not a 'curf' segmented curve");
        return;
    }
    let n = be16(s, 8).unwrap() as usize;
    if be16(s, 10) != Some(0) || be32(s, 4) != Some(0) {
        c.shall(cl, tg, "segmented curve reserved bytes are not zero");
    }
    if n == 0 {
        c.shall(cl, tg, "segmented curve with no segments");
        return;
    }
    let mut prev = f32::NEG_INFINITY;
    for k in 0..n - 1 {
        let Some(bp) = f32be(s, 12 + 4 * k) else {
            c.shall(cl, tg, "break-points truncated");
            return;
        };
        if !finite_normal(bp) {
            c.shall(
                "ICC.1:2022 4.3",
                tg,
                "break-point is NaN, infinite or denormal",
            );
        }
        if bp < prev {
            c.shall(cl, tg, "break-points decrease");
        }
        prev = bp;
    }
    let mut p = 12 + 4 * (n - 1);
    for k in 0..n {
        let Some(sty) = be32(s, p) else {
            c.shall(cl, tg, "segments truncated");
            return;
        };
        let last = k == n - 1;
        match &sty.to_be_bytes() {
            b"parf" => {
                let ft = be16(s, p + 8).unwrap_or(99);
                let np = match ft {
                    0 => 4,
                    1 | 2 => 5,
                    _ => {
                        c.shall(
                            cl,
                            tg,
                            format!("formula segment function type {ft} not in Table 60"),
                        );
                        return;
                    }
                };
                if (0..np).any(|j| f32be(s, p + 12 + 4 * j).is_none_or(|v| !finite_normal(v))) {
                    c.shall(
                        "ICC.1:2022 4.3",
                        tg,
                        "formula parameter missing, NaN, infinite or denormal",
                    );
                }
                p += 12 + 4 * np;
            }
            b"samf" => {
                if k == 0 || last {
                    c.shall(cl, tg, "first or last segment is sampled, not a formula");
                }
                let cnt = be32(s, p + 8).unwrap_or(0) as usize;
                if cnt == 0 {
                    c.shall(
                        "ICC.1:2022 10.16.2.2",
                        tg,
                        "sampled segment with no entries",
                    );
                }
                p += 12 + 4 * cnt;
            }
            _ => {
                c.shall(cl, tg, format!("segment type '{}'", sig_str(sty)));
                return;
            }
        }
    }
    if p > s.len() {
        c.shall(cl, tg, "segments run past the curve");
    }
}

fn check_ncl2(c: &mut Checker, ctx: Ctx, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.17";
    if d.len() < 84 {
        c.shall(cl, tg, "truncated");
        return;
    }
    let (n, m) = (be32(d, 12).unwrap() as usize, be32(d, 16).unwrap() as usize);
    check_name32(c, tg, &d[20..52], cl, false);
    check_name32(c, tg, &d[52..84], cl, false);
    let rec = 38 + 2 * m;
    if d.len() != 84 + n * rec {
        c.shall(
            cl,
            tg,
            format!(
                "{} bytes for {n} colours of {m} device coordinates",
                d.len()
            ),
        );
        return;
    }
    for k in 0..n {
        check_name32(c, tg, &d[84 + k * rec..84 + k * rec + 32], cl, false);
    }
    if m != 0 && channels(ctx.cs) != Some(m as u32) {
        c.should(
            cl,
            tg,
            format!(
                "{m} device coordinates, data colour space has {:?}",
                channels(ctx.cs)
            ),
        );
    }
}

/// An embedded text-description tag (mluc in v4, desc in v2) inside pseq.
fn check_embedded_text(c: &mut Checker, tg: Option<u32>, d: &[u8]) -> usize {
    match be32(d, 0).map(|v| v.to_be_bytes()) {
        Some(b) if &b == b"mluc" => check_mluc(c, tg, d),
        Some(b) if &b == b"desc" => check_text_description(c, tg, d),
        other => {
            c.shall(
                "ICC.1:2022 10.19",
                tg,
                format!(
                    "embedded description of type {:?}",
                    other.map(|b| String::from_utf8_lossy(&b).into_owned())
                ),
            );
            d.len()
        }
    }
}

fn check_pseq(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.19";
    let Some(n) = be32(d, 8) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    let mut p = 12;
    for _ in 0..n {
        if p + 20 > d.len() {
            c.shall(cl, tg, "profile description structure truncated");
            return;
        }
        p += 20;
        for _ in 0..2 {
            if p >= d.len() {
                c.shall(cl, tg, "embedded description missing");
                return;
            }
            // Table 70 (v2: Table 56) places the descriptions back to
            // back. Zero bytes up to a 4-byte boundary followed by a valid
            // type signature mean the writer padded them.
            if be32(d, p).is_none_or(|t| t != sig(b"mluc") && t != sig(b"desc")) {
                let q = align4(p);
                if q != p
                    && d[p..q.min(d.len())].iter().all(|&b| b == 0)
                    && be32(d, q).is_some_and(|t| t == sig(b"mluc") || t == sig(b"desc"))
                {
                    c.shall(
                        cl,
                        tg,
                        "pad bytes between embedded descriptions (Table 70 has none)",
                    );
                    p = q;
                }
            }
            let l = check_embedded_text(c, tg, &d[p..]);
            p += l;
        }
    }
    if p != d.len() && !(align4(p) == d.len() && d[p..].iter().all(|&b| b == 0)) {
        c.shall(
            "ICC.1:2022 7.3.5",
            tg,
            format!("tag size {} but the structure ends at {p}", d.len()),
        );
    } else if p != d.len() {
        c.shall(
            "ICC.1:2022 7.3.5",
            tg,
            "tag size includes trailing pad bytes",
        );
    }
}

fn check_psid(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.20";
    let Some(n) = be32(d, 8) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    let n = n as usize;
    if 12 + 8 * n > d.len() {
        c.shall(cl, tg, "positions table runs past the tag");
        return;
    }
    for k in 0..n {
        let (o, s) = (
            be32(d, 12 + 8 * k).unwrap() as usize,
            be32(d, 16 + 8 * k).unwrap() as usize,
        );
        if o % 4 != 0 || o < 12 + 8 * n || o + s > d.len() || s < 16 + 16 {
            c.shall(
                cl,
                tg,
                format!("identifier {k} at {o}+{s} misaligned or outside the tag"),
            );
            continue;
        }
        let e = &d[o + 16..o + s];
        if be32(e, 0) != Some(sig(b"mluc")) {
            c.shall("ICC.1:2022 10.20.3", tg, "profile description is not mluc");
        } else {
            check_mluc(c, tg, e);
        }
    }
}

fn check_dict(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.9";
    let (Some(m), Some(rl)) = (be32(d, 8), be32(d, 12)) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    if !matches!(rl, 16 | 24 | 32) {
        c.shall(cl, tg, format!("record length {rl}"));
        return;
    }
    let (m, rl) = (m as usize, rl as usize);
    let store = 16 + m * rl;
    if store > d.len() {
        c.shall(cl, tg, "records run past the tag");
        return;
    }
    let mut names = BTreeSet::new();
    for k in 0..m {
        let r = 16 + k * rl;
        let fields = rl / 8;
        for f in 0..fields {
            let (o, s) = (
                be32(d, r + 8 * f).unwrap() as usize,
                be32(d, r + 8 * f + 4).unwrap() as usize,
            );
            let what = ["name", "value", "display name", "display value"][f];
            if o == 0 {
                if f == 0 {
                    c.shall(cl, tg, format!("record {k} has no name string"));
                }
                if s != 0 {
                    c.should(
                        cl,
                        tg,
                        format!("record {k} {what}: zero offset with size {s}"),
                    );
                }
                continue;
            }
            if o % 4 != 0 {
                c.shall(
                    cl,
                    tg,
                    format!("record {k} {what} offset {o} is not a multiple of 4"),
                );
            }
            if o < store || o + s > d.len() {
                c.shall(
                    cl,
                    tg,
                    format!("record {k} {what} {o}+{s} outside the storage area"),
                );
                continue;
            }
            let item = &d[o..o + s];
            match f {
                0 | 1 => {
                    if s % 2 != 0 {
                        c.shall(
                            cl,
                            tg,
                            format!("record {k} {what} is not whole UTF-16 units"),
                        );
                    }
                    if s >= 2 && item[s - 2] == 0 && item[s - 1] == 0 {
                        c.shall(cl, tg, format!("record {k} {what} is NUL-terminated"));
                    }
                    if f == 0 {
                        if s == 0 {
                            c.shall(cl, tg, format!("record {k} name string is empty"));
                        }
                        if !names.insert(item.to_vec()) {
                            c.shall(cl, tg, format!("record {k} name string is not unique"));
                        }
                    }
                }
                _ => {
                    if s > 0 {
                        if be32(item, 0) != Some(sig(b"mluc")) {
                            c.shall(cl, tg, format!("record {k} {what} is not mluc"));
                        } else {
                            check_mluc(c, tg, item);
                        }
                    }
                }
            }
            let pad = &d[(o + s).min(d.len())..align4(o + s).min(d.len())];
            if pad.iter().any(|&b| b != 0) {
                c.shall(cl, tg, format!("record {k} {what} pad bytes are not zero"));
            }
        }
    }
}

fn check_rcs2(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2022 10.21";
    let (Some(n), Some(m)) = (be16(d, 8), be16(d, 10)) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    let (n, m) = (n as usize, m as usize);
    for k in 0..m {
        let Some(o) = be32(d, 12 + 4 * k) else {
            c.shall(cl, tg, "offsets truncated");
            return;
        };
        let o = o as usize;
        if o + 4 + 16 * n > d.len() {
            c.shall(cl, tg, format!("curve structure {k} runs past the tag"));
            continue;
        }
        let counts: Vec<usize> = (0..n)
            .map(|i| be32(d, o + 4 + 4 * i).unwrap() as usize)
            .collect();
        let mut q = o + 4 + 16 * n;
        for &cnt in &counts {
            for _ in 0..cnt {
                if q + 8 > d.len() {
                    c.shall(cl, tg, "response array runs past the tag");
                    return;
                }
                if be16(d, q + 2) != Some(0) {
                    c.shall(
                        "ICC.1:2022 4.5",
                        tg,
                        "response16Number reserved bytes are not zero",
                    );
                }
                q += 8;
            }
        }
    }
}

fn check_ucrbg(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2001-04 6.5.20";
    let Some(u) = be32(d, 8) else {
        c.shall(cl, tg, "truncated");
        return;
    };
    let p = 12 + 2 * u as usize;
    let Some(b) = be32(d, p) else {
        c.shall(cl, tg, "BG count missing");
        return;
    };
    let s = p + 4 + 2 * b as usize;
    if s >= d.len() || d[d.len() - 1] != 0 {
        c.shall(cl, tg, "description string missing or not NUL-terminated");
    }
}

fn check_crdi(c: &mut Checker, tg: Option<u32>, d: &[u8]) {
    let cl = "ICC.1:2001-04 6.5.2";
    let mut p = 8;
    for k in 0..5 {
        let Some(n) = be32(d, p) else {
            c.shall(cl, tg, format!("string {k} count missing"));
            return;
        };
        let n = n as usize;
        if n > 0 && (p + 4 + n > d.len() || d[p + 4 + n - 1] != 0) {
            c.shall(cl, tg, format!("string {k} is not NUL-terminated"));
            return;
        }
        p += 4 + n;
    }
}
