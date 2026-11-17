# ICC and colorimetry conformance

What the port writes and computes, checked against the published
specifications. The port is bit-exact with lcms2.20, so this is also an audit
of lcms2.20: where lcms2 departs from a specification, the port departs the
same way, and that is listed here rather than changed.

Run it: `just conformance` (part of the full test suite). Independent
validation: `just icc-validate` (needs ICC's iccDEV `iccDumpProfile`).

## Sources

From `imazen-private/standards-ark` (`icc/`, `itu-t/`) and the zenpapers
corpus, at the latest revisions: zenpapers `docs/standards-and-references.md`
records a 2026-11-16 check of color.org that the base specifications are
still ICC.1:2022 and ICC.2:2023, with the amendments as the later changes.

| Text | Used for |
|---|---|
| ICC.1:2022-05, profile version 4.4.0.0 | clauses 4, 6.3.4, 6.4, 7, 8, 9, 10; Annex E; Annex F (normative) |
| ICC.1:2001-04, profile version 2.4.0 | clauses 6.1 to 6.5, for profiles the port writes as v2 |
| ICC.1 amendment: cicpType ColourPrimaries = 2 chromaticities (approved 2026-10-19) | the 44-byte cicpType |
| ICC.1 amendment: Headroom Adaptive Gain Curve (approved 2026-10-19) | the 'HAGC' tag |
| ICC 3-component colour encoding registry: sRGB | the built-in sRGB profile |
| ICC technical notes: populating v4 matrix entries; applying partial adaptation | read; neither adds a requirement lcms2 is subject to (the first is guidance for profile builders, the second concerns iccMAX PCC tags) |
| Sharma, Wu, Dalal (2005), *The CIEDE2000 color-difference formula: implementation notes, supplementary test data, and mathematical observations*, doi:10.1002/col.20070 (zenpapers corpus) | Table I test data |
| ITU-T H.273 (2024) | cicpType field values |

Formulas were read from the PDF pages: both text extractions (the archive's
Markdown and `pdftotext -layout`) lose maths symbols, for example the
comparison operators in Table 68.

## Method

1. `tests/support/icc_spec.rs` is a checker written from the clauses, not
   from lcms. It parses profile bytes itself and reports each broken rule
   with its clause: header fields (7.2), tag table layout, alignment,
   padding and sharing (7.1.2, 7.3, 7.4; v2: 6.2), required tags per class
   and version (8; v2: 6.3), the permitted tag types per tag for v2 and v4
   (9.2; v2: 6.4), and the byte layout of every type the port writes
   (10; v2: 6.5).
2. `tests/conformance_icc.rs` runs it over every profile the port can
   produce, each also produced by the C oracle:
   - every generator (sRGB, Lab v2/v4, XYZ, gray, custom RGB, NULL, BCHSW,
     linearization, ink-limiting and transform device links, 8-bit links),
     as built, saved as v2.1, and with a profile ID;
   - a profile carrying every tag type the port writes, a NamedColor profile,
     in v4.4 and v2.1, re-encoded by each side's writers;
   - the 23 testbed profiles both sides can open (all but the deliberately
     malformed `bad.icc`, `bad_mpe.icc` and `toosmall.icc`), re-encoded by
     each side. Findings the input already has are not counted against the
     writers.

   Cases neither side can write (the NULL profile as v2.1, the OkLab
   profile, `toosmall.icc`) are listed in the test's `BOTH_FAIL`; any other
   refusal fails the test.

   A finding only the port has is a port bug and fails the test. A finding
   both have is lcms2 behaviour; each must be listed in the test's
   `KNOWN_LCMS2_DEVIATIONS` with an ID from the table below.
3. The same test file checks the port's arithmetic against values printed in
   the specifications (see "Arithmetic").
4. ICC's own validator, iccDEV `iccDumpProfile -v` (IccProfLib 2.4.2.6,
   commit 4a9ca21), was run on all 92 port/C profile pairs: its verdict was
   identical for the port and C in all 92.

## Result

- **Port vs lcms2.20:** identical findings on all 92 cases, identical iccDEV
  verdicts.
- **One port bug, fixed:** profiles the port created had an all-zero
  creation date (`0-0-0 0:0:0`), which ICC.1:2022 7.2.8 / 4.2 forbid; C
  stamps the current UTC time (`_cmsGetTime`). The port now stamps UTC the
  same way. The byte-parity tests masked the date, which hid it.
- **Two `lcms::api` / `lcms::lcms2` bugs, fixed:** writing a colorant table
  ('clrt', 'clot') from a named-colour list failed to save, through both
  facades (C uses one list type for 'ncl2', 'clrt' and 'clot'); and
  `api::IccData::text` stored ASCII data without the NUL that 10.7
  requires (it now adds it and rejects non-ASCII input). The `lcms2` shim's
  `TagSignature` now has lcms2-sys 4.0.7's 70 variants (it had 22, two of
  them misspelled `ATToB1Tag`/`ATToB2Tag`).
- **lcms2.20 vs the specifications:** the deviations below.

### Deviations in lcms2.20 (reproduced by the port)

| ID | Clause | What lcms2 writes | C source |
|---|---|---|---|
| D1 | 8.6; v2 6.3.4.1 | Device links from `cmsTransform2DeviceLink` carry the required 'pseq' only when the transform was created with `cmsFLAGS_KEEP_SEQUENCE`. Pass `api::flags::KEEP_SEQUENCE` to `Plan::build` for conformant links. | cmsvirt.c:1345 |
| D2 | 8.5.2; 10.13.1 | `cmsCreateNULLProfile` is an Output profile with only 'B2A0', and that lutBToAType maps 3 channels to 1 through a matrix with no CLUT. A gamut-check sink, not a profile to distribute. | cmsvirt.c:964 |
| D3 | 9.2.36 | `cmsCreateGrayProfile` stores a non-D50 white point unadapted as 'wtpt' of a v4 Display profile; v4 requires the PCS illuminant there (`cmsCreateRGBProfile` does this right, with 'chad'). | cmsvirt.c:227 |
| D4 | v2 6.2.3; 10.19; 7.3.5 | textDescriptionType is padded to 4 bytes inside the type. v2 'desc', 'dmnd', 'dmdd', 'vued' tag sizes include the padding, and v2 'pseq' descriptions are separated by pad bytes that Table 70 does not have. iccDEV flags these 'pseq' tags NonCompliant too. | cmstypes.c:1219 |
| D5 | 10.20 | profileSequenceIdentifierType structures are not padded to 4-byte boundaries. | cmstypes.c:290 |
| D6 | 7.3.1; 10.20.3 | Saving a v4 profile as v2 keeps v4-only tags such as 'psid', whose descriptions then come out as textDescriptionType instead of mluc. | (no version filter on save) |
| D7 | 10.10 | A curves-only pipeline saved as lut16Type gets a CLUT with 0 grid points; the lut16 model always applies its CLUT. iccDEV rejects these tags. | cmstypes.c:2420 |
| D8 | 10.15 | v2 text read into an MLU keeps lcms's internal 0xFFFF "v2 Unicode" locale, which then appears as language/country 0xFFFF in v4 mluc records (e.g. 'pseq'/'psid' of a link built from v2 profiles). | cmstypes.c:1178 |
| D9 | v2 6.1 | `cmsMD5computeID` also fills the reserved header bytes of v2 profiles (the profile ID is a v4 field). | cmsmd5.c:257 |
| D10 | 7.2.18 (should) | No profile ID unless `cmsMD5computeID` is called. | — |
| D11 | 10.9 | dictType strings and display elements are not padded to 4-byte boundaries. | cmstypes.c:5391 |
| D12 | 10.9 | dictType display name/value elements are bare mluc bodies, without the 'mluc' type signature and reserved field ("encoded as a multiLocalizedUnicodeType tag"). lcms reads them the same way, so it misreads conformant dictionaries and other readers misread lcms's: iccDEV rejects the tag. | cmstypes.c:5430 |
| D13 | 10.18 (Table 68) | Float evaluation of parametric curves is not clipped to [0, 1] ("any function value outside the range shall be clipped"). The 16-bit path clips. lcms2's float transforms are unbounded by design. | cmsgamma.c:344 |
| D14 | Annex F.1 a) (normative) | `cmsReverseToneCurve` does not invert flat stretches as F.1 says: a stretch over [0.3, 0.6] inverts to 0.459 (F.1: its highest x, 0.6), and one reaching the end of the domain, [0.5, 1], inverts to 1.0 (F.1: its lowest x, 0.5). | cmsgamma.c:1070 |
| D15 | cicp amendment 2026-10-19 | The 44-byte cicpType (ColourPrimaries 2 plus eight chromaticities) is rejected: lcms2.20 predates the amendment and requires exactly 12 bytes. | cmstypes.c:5661 |

D1 to D12 are enforced by `KNOWN_LCMS2_DEVIATIONS`; D13 to D15 are pinned by
`parametric_curves_match_table_68`, `curve_inversion_against_annex_f1` and
`cicp_with_2026_chromaticity_extension`, so a change in lcms2 (or the port)
fails a test.

### Conformant, as checked

- Header (7.2): every field with a constraint (size, version, class,
  colour spaces, date, 'acsp', platform, flags, attributes, intent, PCS
  illuminant D50, profile ID, reserved bytes), for every written profile;
  dates are UTC.
- Tag table and tag data (7.1.2, 7.3, 7.4): 4-byte alignment, contiguity, NUL
  padding, sizes, shared data.
- Tag types per tag (9.2; v2 6.4): every tag the generators write, except D6.
- Type layouts (10; v2 6.5): 21 of the 30 ICC.1:2022 tag types appear in
  checked v4 profiles. The other 9: lut8Type/lut16Type, which lcms2 writes
  only in v2 profiles (checked there, except D7); dataType, which no v4 tag
  uses (checked in v2); responseCurveSet16Type, which the port reads but has
  no writer for; and the five ui*/uf32 array types, which no ICC.1 tag uses.
  Across both versions 27 ICC types are checked (plus Apple's private
  'vcgt'), including the v2-only textDescriptionType, screeningType and
  ucrbgType.
- Required tags (8; v2 6.3): every generator except D1 and D2.
- Unknown tags (for example the amendment's 'HAGC') survive open/save
  byte for byte, through the parity layer, C and `lcms::api`.

### Arithmetic

| Check | Reference | Result |
|---|---|---|
| s15Fixed16, u8Fixed8 encodings | ICC.1:2022 Tables 4, 7 | exact |
| PCSXYZ u1Fixed15, PCSLAB v4 and legacy encodings, white, absorber, perceptual black | Tables 6, 11 to 16, 42, 43 | exact; integer encodings clip (6.3.4.1) |
| Parametric curves, function types 0 to 4 | Table 68 | formulas within 2e-6; see D13 |
| multiProcessElements formula segments | Table 60 | within 1e-5 relative |
| Chromatic adaptation | Annex E, linear Bradford (E.1 to E.4) | within 1e-12 |
| Profile ID | 7.2.18 / RFC 1321 (independent `md-5` crate) | exact |
| mluc record lookup fallback | 10.15 | as specified |
| Built-in sRGB profile | ICC sRGB registry entry + Annex E | colorants within 2/65536, TRC within 1e-6, 'wtpt' D50 |
| CIEDE2000 | Sharma et al. 2005, Table I, 34 pairs | all within 5e-5, symmetric |

lcms2.h's perceptual black point (0.00336, 0.0034731, 0.00287) differs from
Table 16's PCSXYZ values (0.003357, 0.003479, 0.002869) in the last digits:
X and Z are rounded to three significant figures, and Y equals Table 16's
L* 3.1373 through the CIE linear segment (3.1373 / 903.3); the table's note
allows Y and L* to disagree. All three encode to Table 16's 16-bit values
(006Eh, 0072h, 005Eh).

## Not verified

- CIECAM02 (`cam02`): CIE 159:2004 is not archived. The archived
  Schlömer paper (arXiv:1802.06067) gives the formulas and CAM16 edge cases
  but no CIECAM02 reference outputs.
- CIE daylight locus and correlated colour temperature
  (`cmsWhitePointFromTemp`, `cmsTempFromWhitePoint`): CIE 015:2018 is
  paywalled (zenpapers lists it so) and not archived.
- CGATS / IT8.7 text files (`cgats`): ANSI CGATS.17 is not archived.
- PostScript CSA/CRD generation (`ps2`): the PostScript Language Reference is
  not archived.
- Black-point compensation: ISO 18619 is not archived; ICC White Paper #40
  describes the algorithm without test values.
- ICC.2 (iccMAX): lcms2.20 implements only the ICC.1 multiProcessElements
  subset; the partial-adaptation technical note is about iccMAX's PCC tags
  and does not apply.

These modules are still bit-exact with lcms2.20 (their `c_parity_*` gates).

## Where iccDEV disagrees

A disagreement is settled against the specification text:

- A one-entry 'pseq' (the linearization and ink-limiting links) is rejected
  as "invalid structure": iccDEV requires 12 bytes plus
  `sizeof(CIccProfileDescStruct)`, an in-memory C++ size
  (`CIccTagProfileSeqDesc::Read`). The 64-byte tag is valid under Table 69/70
  with the placeholder descriptions 10.19 recommends.
- Byte-identical lut16Type data is accepted as 'A2B0' and rejected as 'B2A0'
  ("input tables require 2 to 4096 entries, not 0"); the stored count is
  4096.
- "LinkTag exclusion" warns because lcms's DeviceLinks carry 'wtpt', which
  iccDEV's exclusion list for links contains (`CIccProfile::CheckTagExclusion`).
  ICC.1:2022 8.6 only says a link is not required to have one.
- "Empty tag" warns on the zero-record mluc placeholders that 10.19 itself
  recommends, and language code 0x0000 is called malformed, where 10.1 makes
  0 the "unknown" value of enumerated fields.
- iccDEV expects mluc descriptions inside 'pseq' to start on 4-byte
  boundaries; Table 70 has no padding field and lcms2 writes none for mluc
  (it does pad textDescriptionType: D4). No checked case hit this.

Upstream reports to iccDEV would be posts outside the imazen/lilith orgs and
need the owner's approval.
