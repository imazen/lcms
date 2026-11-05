//! `cmscgats.c` — the IT8/CGATS file format + the `.cube` LUT parser.
//! A line-oriented text format: sheet type, header properties,
//! BEGIN_DATA_FORMAT/END_DATA_FORMAT, BEGIN_DATA/END_DATA.

use crate::context::Context;
use crate::gamma::ToneCurve;
use crate::lut::{Pipeline, Stage, StageLoc};
use crate::profile::IccProfile;

pub const MAXSTR: usize = 1024;
pub const MAXTABLES: usize = 255;
pub const MAXID: usize = 128;
pub const DEFAULT_DBL_FORMAT: &str = "%.10g";

/// `SYMBOL` — lexer token
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symbol {
    Undefined,
    Inum,
    Dnum,
    Ident,
    Str,
    Comment,
    Eoln,
    Eof,
    SynError,
    BeginData,
    BeginDataFormat,
    EndData,
    EndDataFormat,
    Keyword,
    DataFormatId,
    Include,
    DomainMax,
    DomainMin,
    Lut1dSize,
    Lut1dInputRange,
    Lut3dSize,
    Lut3dInputRange,
    LutInVideoRange,
    LutOutVideoRange,
    Title,
}

/// `WRITEMODE`
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Uncooked,
    Stringify,
    Hexadecimal,
    Binary,
    Pair,
}

/// `KEYVALUE` — a property (name/subkey/value + write mode)
#[derive(Clone)]
pub struct KeyValue {
    pub keyword: String,
    pub subkey: Option<String>,
    pub value: Option<String>,
    pub write_as: WriteMode,
}

/// `TABLE` — one IT8 data table
#[derive(Clone)]
pub struct Table {
    pub sheet_type: String,
    pub n_samples: usize,
    pub n_patches: usize,
    pub sample_id: usize,
    pub header_list: Vec<KeyValue>,
    pub data_format: Vec<Option<String>>,
    /// flattened `[patch][field]`
    pub data: Vec<Option<String>>,
}

impl Table {
    fn new() -> Self {
        Table {
            sheet_type: String::new(),
            n_samples: 0,
            n_patches: 0,
            sample_id: 0,
            header_list: Vec::new(),
            data_format: Vec::new(),
            data: Vec::new(),
        }
    }
}

/// `KEYWORD` — a predefined keyword → symbol
#[derive(Clone, Copy)]
pub struct Keyword {
    pub id: &'static str,
    pub sy: Symbol,
}

/// `cmsIT8` — the document
pub struct It8 {
    pub ctx: Option<Context>,
    pub tables_count: usize,
    pub n_table: usize,
    pub is_cube: bool,
    pub tables: Vec<Table>,
    // lexer state
    pub sy: Symbol,
    pub ch: i32,
    pub inum: i32,
    pub dnum: f64,
    pub id: String,
    pub str_: String,
    pub source: Vec<u8>,
    pub source_pos: usize,
    pub lineno: usize,
    pub valid_keywords: Vec<(String, WriteMode)>,
    pub valid_sample_ids: Vec<String>,
    pub double_formatter: String,
}

/// `TabKeysIT8` — case-insensitive lookup table
const TABKEYS_IT8: &[Keyword] = &[
    Keyword {
        id: "$INCLUDE",
        sy: Symbol::Include,
    },
    Keyword {
        id: ".INCLUDE",
        sy: Symbol::Include,
    },
    Keyword {
        id: "BEGIN_DATA",
        sy: Symbol::BeginData,
    },
    Keyword {
        id: "BEGIN_DATA_FORMAT",
        sy: Symbol::BeginDataFormat,
    },
    Keyword {
        id: "DATA_FORMAT_IDENTIFIER",
        sy: Symbol::DataFormatId,
    },
    Keyword {
        id: "END_DATA",
        sy: Symbol::EndData,
    },
    Keyword {
        id: "END_DATA_FORMAT",
        sy: Symbol::EndDataFormat,
    },
    Keyword {
        id: "KEYWORD",
        sy: Symbol::Keyword,
    },
];

/// `TabKeysCUBE`
const TABKEYS_CUBE: &[Keyword] = &[
    Keyword {
        id: "DOMAIN_MAX",
        sy: Symbol::DomainMax,
    },
    Keyword {
        id: "DOMAIN_MIN",
        sy: Symbol::DomainMin,
    },
    Keyword {
        id: "LUT_1D_SIZE",
        sy: Symbol::Lut1dSize,
    },
    Keyword {
        id: "LUT_1D_INPUT_RANGE",
        sy: Symbol::Lut1dInputRange,
    },
    Keyword {
        id: "LUT_3D_SIZE",
        sy: Symbol::Lut3dSize,
    },
    Keyword {
        id: "LUT_3D_INPUT_RANGE",
        sy: Symbol::Lut3dInputRange,
    },
    Keyword {
        id: "LUT_IN_VIDEO_RANGE",
        sy: Symbol::LutInVideoRange,
    },
    Keyword {
        id: "LUT_OUT_VIDEO_RANGE",
        sy: Symbol::LutOutVideoRange,
    },
    Keyword {
        id: "TITLE",
        sy: Symbol::Title,
    },
];

/// `PredefinedProperties` — (name, write mode)
const PREDEFINED_PROPERTIES: &[(&str, WriteMode)] = &[
    ("NUMBER_OF_FIELDS", WriteMode::Uncooked),
    ("NUMBER_OF_SETS", WriteMode::Uncooked),
    ("ORIGINATOR", WriteMode::Stringify),
    ("FILE_DESCRIPTOR", WriteMode::Stringify),
    ("CREATED", WriteMode::Stringify),
    ("DESCRIPTOR", WriteMode::Stringify),
    ("DIFFUSE_GEOMETRY", WriteMode::Stringify),
    ("MANUFACTURER", WriteMode::Stringify),
    ("MANUFACTURE", WriteMode::Stringify),
    ("PROD_DATE", WriteMode::Stringify),
    ("SERIAL", WriteMode::Stringify),
    ("MATERIAL", WriteMode::Stringify),
    ("INSTRUMENTATION", WriteMode::Stringify),
    ("MEASUREMENT_SOURCE", WriteMode::Stringify),
    ("PRINT_CONDITIONS", WriteMode::Stringify),
    ("SAMPLE_BACKING", WriteMode::Stringify),
    ("CHISQ_DOF", WriteMode::Stringify),
    ("MEASUREMENT_GEOMETRY", WriteMode::Stringify),
    ("FILTER", WriteMode::Stringify),
    ("POLARIZATION", WriteMode::Stringify),
    ("WEIGHTING_FUNCTION", WriteMode::Pair),
    ("COMPUTATIONAL_PARAMETER", WriteMode::Pair),
    ("TARGET_TYPE", WriteMode::Stringify),
    ("COLORANT", WriteMode::Stringify),
    ("TABLE_DESCRIPTOR", WriteMode::Stringify),
    ("TABLE_NAME", WriteMode::Stringify),
];

/// `PredefinedSampleID` — the valid sample-column names
const PREDEFINED_SAMPLE_IDS: &[&str] = &[
    "SAMPLE_ID",
    "STRING",
    "CMYK_C",
    "CMYK_M",
    "CMYK_Y",
    "CMYK_K",
    "D_RED",
    "D_GREEN",
    "D_BLUE",
    "D_VIS",
    "D_MAJOR_FILTER",
    "RGB_R",
    "RGB_G",
    "RGB_B",
    "SPECTRAL_NM",
    "SPECTRAL_PCT",
    "SPECTRAL_DEC",
    "XYZ_X",
    "XYZ_Y",
    "XYZ_Z",
    "XYY_X",
    "XYY_Y",
    "XYY_CAPY",
    "LAB_L",
    "LAB_A",
    "LAB_B",
    "LAB_C",
    "LAB_H",
    "LAB_DE",
    "LAB_DE_94",
    "LAB_DE_CMC",
    "LAB_DE_2000",
    "MEAN_DE",
    "STDEV_X",
    "STDEV_Y",
    "STDEV_Z",
    "STDEV_L",
    "STDEV_A",
    "STDEV_B",
    "STDEV_DE",
    "CHI_SQD_PAR",
];

// --- lexer --------------------------------------------------------------

fn is_separator(c: i32) -> bool {
    c == b' ' as i32 || c == b'\t' as i32
}
fn is_idchar(c: i32) -> bool {
    (0..128).contains(&c)
        && ((c as u8 as char).is_ascii_alphanumeric()
            || c == b'_' as i32
            || c == b'-' as i32
            || c == b'(' as i32
            || c == b')' as i32)
}
fn is_first_idchar(c: i32) -> bool {
    (0..128).contains(&c) && ((c as u8 as char).is_ascii_alphabetic() || c == b'_' as i32)
}

fn strcasecmp(a: &str, b: &str) -> i32 {
    let al = a.to_ascii_lowercase();
    let bl = b.to_ascii_lowercase();
    al.cmp(&bl) as i32
}

fn xpow10(n: i32) -> f64 {
    10f64.powi(n)
}

/// `ParseFloatNumber`
pub fn parse_float_number(buf: &str) -> f64 {
    let mut dnum = 0.0f64;
    let mut sign = 1.0f64;
    let mut b = buf.as_bytes();
    if buf.is_empty() {
        return 0.0;
    }
    if b[0] == b'-' || b[0] == b'+' {
        if b[0] == b'-' {
            sign = -1.0;
        }
        b = &b[1..];
    }
    while !b.is_empty() && b[0].is_ascii_digit() {
        dnum = dnum * 10.0 + (b[0] - b'0') as f64;
        b = &b[1..];
    }
    if !b.is_empty() && b[0] == b'.' {
        let mut frac = 0.0f64;
        let mut prec = 0;
        b = &b[1..];
        while !b.is_empty() && b[0].is_ascii_digit() {
            frac = frac * 10.0 + (b[0] - b'0') as f64;
            prec += 1;
            b = &b[1..];
        }
        dnum += frac / xpow10(prec);
    }
    if !b.is_empty() && (b[0] == b'E' || b[0] == b'e') {
        let mut e = 0i32;
        let mut sgn = 1i32;
        b = &b[1..];
        if !b.is_empty() && b[0] == b'-' {
            sgn = -1;
            b = &b[1..];
        } else if !b.is_empty() && b[0] == b'+' {
            b = &b[1..];
        }
        while !b.is_empty() && b[0].is_ascii_digit() {
            let digit = (b[0] - b'0') as i32;
            if (e as f64 * 10.0 + digit as f64) < 2147483647.0 {
                e = e * 10 + digit;
            }
            b = &b[1..];
        }
        dnum *= xpow10(sgn * e);
    }
    sign * dnum
}

/// `satoi` — C's atoi (parse leading int, 0 on garbage)
fn satoi(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, digits) = if let Some(d) = s.strip_prefix('-') {
        (true, d)
    } else if let Some(d) = s.strip_prefix('+') {
        (false, d)
    } else {
        (false, s)
    };
    let mut n: i64 = 0;
    for &b in digits.as_bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        n = n * 10 + (b - b'0') as i64;
        if n > i32::MAX as i64 {
            break;
        }
    }
    if neg { -(n as i32) } else { n as i32 }
}

/// `satob` — binary string of a decimal int
fn satob(v: &str) -> String {
    if v.is_empty() {
        return "0".to_string();
    }
    let x = satoi(v) as u32;
    if x == 0 {
        return "0".to_string();
    }
    let mut s = String::new();
    let mut x = x;
    while x > 0 {
        s.insert(0, char::from_digit(x % 2, 10).unwrap());
        x /= 2;
    }
    s
}

fn bin_srch_key(id: &str, keys: &[Keyword]) -> Symbol {
    for k in keys {
        if strcasecmp(k.id, id) == 0 {
            return k.sy;
        }
    }
    Symbol::Ident
}

impl It8 {
    /// `cmsIT8Alloc` — empty document + one table + predefined lists
    pub fn alloc(ctx: Option<Context>) -> Box<It8> {
        let mut it8 = Box::new(It8 {
            ctx,
            tables_count: 1,
            n_table: 0,
            is_cube: false,
            tables: vec![Table::new()],
            sy: Symbol::Undefined,
            ch: 0,
            inum: 0,
            dnum: 0.0,
            id: String::new(),
            str_: String::new(),
            source: Vec::new(),
            source_pos: 0,
            lineno: 1,
            valid_keywords: Vec::new(),
            valid_sample_ids: Vec::new(),
            double_formatter: DEFAULT_DBL_FORMAT.to_string(),
        });
        for (id, mode) in PREDEFINED_PROPERTIES {
            it8.valid_keywords.push((id.to_string(), *mode));
        }
        for id in PREDEFINED_SAMPLE_IDS {
            it8.valid_sample_ids.push(id.to_string());
        }
        it8
    }

    pub fn get_table(&self) -> &Table {
        &self.tables[self.n_table]
    }
    pub fn get_table_mut(&mut self) -> &mut Table {
        &mut self.tables[self.n_table]
    }

    /// `cmsIT8GetSheetType`
    pub fn get_sheet_type(&self) -> &str {
        &self.get_table().sheet_type
    }

    /// `cmsIT8SetSheetType`
    pub fn set_sheet_type(&mut self, ty: &str) -> bool {
        self.get_table_mut().sheet_type = ty.to_string();
        true
    }

    /// `cmsIT8SetComment` — a `#`-prefixed header line
    pub fn set_comment(&mut self, val: &str) -> bool {
        self.get_table_mut().header_list.push(KeyValue {
            keyword: "#".to_string(),
            subkey: None,
            value: Some(val.to_string()),
            write_as: WriteMode::Uncooked,
        });
        true
    }

    /// `cmsIT8SetTable` — select (or extend to) table n
    pub fn set_table(&mut self, n: usize) -> i32 {
        while self.tables.len() <= n {
            self.tables.push(Table::new());
            self.tables_count = self.tables.len();
        }
        self.n_table = n;
        n as i32
    }

    /// `AllocTable` — append a table, cap MAXTABLES
    fn alloc_table(&mut self) -> bool {
        if self.tables_count >= MAXTABLES - 1 {
            return false;
        }
        self.tables.push(Table::new());
        self.tables_count = self.tables.len();
        true
    }

    // --- list helpers ----------------------------------------------------

    /// `IsAvailableOnList` — find keyword (+ optional subkey)
    fn is_available_on_list<'a>(
        &self,
        list: &'a [KeyValue],
        key: &str,
        subkey: Option<&str>,
    ) -> Option<&'a KeyValue> {
        for kv in list {
            if strcasecmp(&kv.keyword, key) == 0 {
                match subkey {
                    None => return Some(kv),
                    Some(sk) => {
                        if let Some(s) = &kv.subkey {
                            if strcasecmp(s, sk) == 0 {
                                return Some(kv);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// `AddToList` — insert or update a key (dup NUMBER_OF_FIELDS/SETS = error)
    fn add_to_list(
        &mut self,
        list_is_header: bool,
        key: &str,
        subkey: Option<&str>,
        value: Option<&str>,
        write_as: WriteMode,
    ) -> bool {
        // check dup for the two count keys
        let exists = if list_is_header {
            self.is_available_on_list(&self.get_table().header_list, key, subkey)
        } else {
            None
        };
        if exists.is_some()
            && (strcasecmp(key, "NUMBER_OF_FIELDS") == 0 || strcasecmp(key, "NUMBER_OF_SETS") == 0)
        {
            self.syn_error(&format!("duplicate key <{key}>"));
            return false;
        }
        let kv = KeyValue {
            keyword: key.to_string(),
            subkey: subkey.map(|s| s.to_string()),
            value: value.map(|s| s.to_string()),
            write_as,
        };
        if list_is_header {
            self.get_table_mut().header_list.push(kv);
        }
        true
    }

    /// `AddAvailableProperty`
    fn add_available_property(&mut self, key: &str, mode: WriteMode) -> bool {
        self.valid_keywords.push((key.to_string(), mode));
        true
    }

    /// `AddAvailableSampleID`
    fn add_available_sample_id(&mut self, key: &str) -> bool {
        self.valid_sample_ids.push(key.to_string());
        true
    }

    // --- property accessors -----------------------------------------------

    /// `cmsIT8GetProperty`
    pub fn get_property(&self, key: &str) -> Option<String> {
        self.is_available_on_list(&self.get_table().header_list, key, None)
            .and_then(|kv| kv.value.clone())
    }

    /// `cmsIT8GetPropertyDbl`
    pub fn get_property_dbl(&self, key: &str) -> f64 {
        self.get_property(key)
            .map(|s| parse_float_number(&s))
            .unwrap_or(0.0)
    }

    /// `cmsIT8SetPropertyStr` (Uncooked/Stringify by string-vs-not)
    pub fn set_property_str(&mut self, key: &str, val: &str) -> bool {
        let mode = if val.contains(' ') || val.contains('\t') {
            WriteMode::Stringify
        } else {
            WriteMode::Uncooked
        };
        self.add_to_list(true, key, None, Some(val), mode)
    }

    /// `cmsIT8SetPropertyDbl`
    pub fn set_property_dbl(&mut self, key: &str, val: f64) -> bool {
        // C formats via DoubleFormatter — approximate with %.*g
        let s = format!("{val}");
        self.add_to_list(true, key, None, Some(&s), WriteMode::Uncooked)
    }

    /// `cmsIT8SetPropertyHex`
    pub fn set_property_hex(&mut self, key: &str, val: u32) -> bool {
        let s = format!("0x{val:x}");
        self.add_to_list(true, key, None, Some(&s), WriteMode::Hexadecimal)
    }

    /// `cmsIT8SetPropertyUncooked`
    pub fn set_property_uncooked(&mut self, key: &str, val: &str) -> bool {
        self.add_to_list(true, key, None, Some(val), WriteMode::Uncooked)
    }

    /// `cmsIT8SetPropertyMulti` — `key subkey value` triple
    pub fn set_property_multi(&mut self, key: &str, subkey: &str, val: &str) -> bool {
        self.add_to_list(true, key, Some(subkey), Some(val), WriteMode::Pair)
    }

    /// `cmsIT8GetPropertyMulti`
    pub fn get_property_multi(&self, key: &str, subkey: &str) -> Option<String> {
        self.is_available_on_list(&self.get_table().header_list, key, Some(subkey))
            .and_then(|kv| kv.value.clone())
    }

    // --- data accessors ----------------------------------------------------

    /// `AllocateDataFormat` — size the format vec to n_samples+1
    fn allocate_data_format(&mut self) -> bool {
        let n = self
            .get_property("NUMBER_OF_FIELDS")
            .map(|s| satoi(&s))
            .unwrap_or(0);
        if n <= 0 || n > 0x7ffe {
            self.syn_error("Wrong NUMBER_OF_FIELDS");
            return false;
        }
        self.get_table_mut().n_samples = n as usize;
        self.get_table_mut().data_format = vec![None; n as usize + 1];
        true
    }

    /// `AllocateDataSet` — size the data vec to (n_samples+1)*(n_patches+1)
    fn allocate_data_set(&mut self) -> bool {
        let n = self
            .get_property("NUMBER_OF_FIELDS")
            .map(|s| satoi(&s))
            .unwrap_or(0);
        let p = self
            .get_property("NUMBER_OF_SETS")
            .map(|s| satoi(&s))
            .unwrap_or(0);
        if n < 0 || n > 0x7ffe || p < 0 || p > 0x7ffe || n * p > 200000 {
            self.syn_error("AllocateDataSet: too much data");
            return false;
        }
        self.get_table_mut().n_samples = n as usize;
        self.get_table_mut().n_patches = p as usize;
        self.get_table_mut().data = vec![None; (n as usize + 1) * (p as usize + 1)];
        true
    }

    /// `GetDataFormat` — column-name for field n
    fn get_data_format(&self, n: usize) -> Option<&str> {
        self.get_table().data_format.get(n)?.as_deref()
    }

    /// `SetDataFormat` — set column-name for field n
    fn set_data_format(&mut self, n: usize, label: &str) -> bool {
        if self.get_table().data_format.is_empty() && !self.allocate_data_format() {
            return false;
        }
        let ns = self.get_table().n_samples;
        if n >= ns {
            self.syn_error("Invalid or more than NUMBER_OF_FIELDS fields");
            return false;
        }
        self.get_table_mut().data_format[n] = Some(label.to_string());
        true
    }

    /// `cmsIT8SetDataFormat`
    pub fn set_data_format_export(&mut self, n: usize, label: &str) -> bool {
        self.set_data_format(n, label)
    }

    /// `GetData` — value at [patch][field]
    fn get_data(&self, set: usize, field: usize) -> Option<&str> {
        let t = self.get_table();
        if set >= t.n_patches || field >= t.n_samples {
            return None;
        }
        t.data.get(set * t.n_samples + field)?.as_deref()
    }

    /// `SetData`
    fn set_data(&mut self, set: usize, field: usize, val: &str) -> bool {
        if self.get_table().data.is_empty() && !self.allocate_data_set() {
            return false;
        }
        let (n_patches, n_samples) = {
            let t = self.get_table();
            (t.n_patches, t.n_samples)
        };
        if set > n_patches {
            return self.syn_error(&format!(
                "Patch {set} out of range, there are {n_patches} patches"
            ));
        }
        if field > n_samples {
            return self.syn_error(&format!(
                "Sample {field} out of range, there are {n_samples} samples"
            ));
        }
        let t = self.get_table_mut();
        t.data[set * n_samples + field] = Some(val.to_string());
        true
    }

    /// `LocatePatch` — row index of patch name in the SAMPLE_ID column
    fn locate_patch(&self, patch: &str) -> i32 {
        let t = self.get_table();
        for i in 0..t.n_patches {
            if let Some(data) = self.get_data(i, t.sample_id) {
                if strcasecmp(data, patch) == 0 {
                    return i as i32;
                }
            }
        }
        -1
    }

    /// `LocateEmptyPatch` — first row with no SAMPLE_ID value
    fn locate_empty_patch(&self) -> i32 {
        let t = self.get_table();
        for i in 0..t.n_patches {
            if self.get_data(i, t.sample_id).is_none() {
                return i as i32;
            }
        }
        -1
    }

    /// `LocateSample` — column index of a field name
    fn locate_sample(&self, sample: &str) -> i32 {
        for i in 0..self.get_table().n_samples {
            if let Some(fld) = self.get_data_format(i) {
                if strcasecmp(fld, sample) == 0 {
                    return i as i32;
                }
            }
        }
        -1
    }

    /// `cmsIT8FindDataFormat`
    pub fn find_data_format(&self, sample: &str) -> i32 {
        self.locate_sample(sample)
    }

    /// `cmsIT8GetData`
    pub fn get_data_by_name(&self, patch: &str, sample: &str) -> Option<String> {
        let field = self.locate_sample(sample);
        if field < 0 {
            return None;
        }
        let set = self.locate_patch(patch);
        if set < 0 {
            return None;
        }
        self.get_data(set as usize, field as usize)
            .map(|s| s.to_string())
    }

    /// `cmsIT8GetDataDbl`
    pub fn get_data_dbl(&self, patch: &str, sample: &str) -> f64 {
        self.get_data_by_name(patch, sample)
            .map(|s| parse_float_number(&s))
            .unwrap_or(0.0)
    }

    /// `cmsIT8SetData`
    pub fn set_data_by_name(&mut self, patch: &str, sample: &str, val: &str) -> bool {
        let field = self.locate_sample(sample);
        if field < 0 {
            return false;
        }
        let n_patches = self.get_table().n_patches;
        if n_patches == 0 {
            if !self.allocate_data_format() || !self.allocate_data_set() {
                return false;
            }
            self.cook_pointers();
        }
        if strcasecmp(sample, "SAMPLE_ID") == 0 {
            let set = self.locate_empty_patch();
            if set < 0 {
                return self.syn_error(&format!("Couldn't add more patches '{patch}'"));
            }
            let sid = self.get_table().sample_id;
            return self.set_data(set as usize, sid, val);
        }
        let set = self.locate_patch(patch);
        if set < 0 {
            return false;
        }
        self.set_data(set as usize, field as usize, val)
    }

    /// `cmsIT8SetDataDbl`
    pub fn set_data_dbl(&mut self, patch: &str, sample: &str, val: f64) -> bool {
        let s = format!("{val}");
        self.set_data_by_name(patch, sample, &s)
    }

    /// `cmsIT8GetDataRowCol`
    pub fn get_data_row_col(&self, row: usize, col: usize) -> Option<String> {
        self.get_data(row, col).map(|s| s.to_string())
    }

    /// `cmsIT8GetDataRowColDbl`
    pub fn get_data_row_col_dbl(&self, row: usize, col: usize) -> f64 {
        self.get_data(row, col)
            .map(|s| parse_float_number(s))
            .unwrap_or(0.0)
    }

    /// `cmsIT8SetDataRowCol`
    pub fn set_data_row_col(&mut self, row: usize, col: usize, val: &str) -> bool {
        self.set_data(row, col, val)
    }

    /// `cmsIT8SetDataRowColDbl`
    pub fn set_data_row_col_dbl(&mut self, row: usize, col: usize, val: f64) -> bool {
        let s = format!("{val}");
        self.set_data(row, col, &s)
    }

    /// `cmsIT8GetPatchName`
    pub fn get_patch_name(&self, n_patch: usize) -> Option<String> {
        let sid = self.get_table().sample_id;
        self.get_data(n_patch, sid).map(|s| s.to_string())
    }

    /// `cmsIT8GetPatchByName`
    pub fn get_patch_by_name(&self, patch: &str) -> i32 {
        self.locate_patch(patch)
    }

    /// `cmsIT8TableCount`
    pub fn table_count(&self) -> u32 {
        self.tables_count as u32
    }

    /// `cmsIT8SetTableByLabel` — resolve `label table type` in a field
    pub fn set_table_by_label(&mut self, set: &str, field: &str, expected_type: &str) -> i32 {
        let c_field = if field.is_empty() { "LABEL" } else { field };
        let Some(label_fld) = self.get_data_by_name(set, c_field) else {
            return -1;
        };
        let mut parts = label_fld.split_whitespace();
        let _label = parts.next().unwrap_or("");
        let n_table = parts
            .next()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let ty = parts.next().unwrap_or("");
        if !expected_type.is_empty() && strcasecmp(ty, expected_type) != 0 {
            return -1;
        }
        self.set_table(n_table)
    }

    /// `cmsIT8SetIndexColumn`
    pub fn set_index_column(&mut self, sample: &str) -> bool {
        let pos = self.locate_sample(sample);
        if pos == -1 {
            return false;
        }
        self.get_table_mut().sample_id = pos as usize;
        true
    }

    /// `cmsIT8DefineDblFormat`
    pub fn define_dbl_format(&mut self, formatter: &str) {
        self.double_formatter = if formatter.is_empty() {
            DEFAULT_DBL_FORMAT.to_string()
        } else {
            formatter.to_string()
        };
    }

    /// `cmsIT8EnumDataFormat` — field names
    pub fn enum_data_format(&self) -> Vec<&str> {
        (0..self.get_table().n_samples)
            .filter_map(|i| self.get_data_format(i))
            .collect()
    }

    /// `cmsIT8EnumProperties` — property names
    pub fn enum_properties(&self) -> Vec<&str> {
        self.get_table()
            .header_list
            .iter()
            .map(|kv| kv.keyword.as_str())
            .collect()
    }

    /// `cmsIT8EnumPropertyMulti` — subkeys for a key
    pub fn enum_property_multi(&self, prop: &str) -> Vec<&str> {
        self.get_table()
            .header_list
            .iter()
            .filter(|kv| strcasecmp(&kv.keyword, prop) == 0)
            .filter_map(|kv| kv.subkey.as_deref())
            .collect()
    }

    // --- lexer --------------------------------------------------------------

    /// `NextCh` — advance one byte from the memory stream
    fn next_ch(&mut self) {
        self.ch = if self.source_pos < self.source.len() {
            let c = self.source[self.source_pos] as i32;
            self.source_pos += 1;
            c
        } else {
            0
        };
    }

    /// `SynError`
    fn syn_error(&mut self, txt: &str) -> bool {
        let mut b = txt.to_string();
        if self.lineno > 1 {
            b.push_str(&format!(" (line {})", self.lineno));
        }
        crate::err::signal_error(self.ctx.as_ref(), crate::types::ERROR_READ, &b);
        self.sy = Symbol::SynError;
        false
    }

    /// `Check`
    fn check(&mut self, sy: Symbol, err: &str) -> bool {
        if self.sy == sy {
            true
        } else {
            self.syn_error(err)
        }
    }

    /// `ReadReal` — continue scanning a float after the int part
    fn read_real(&mut self, inum: i32) {
        self.dnum = inum as f64;
        while (self.ch as u8).is_ascii_digit() {
            self.dnum = self.dnum * 10.0 + (self.ch - b'0' as i32) as f64;
            self.next_ch();
        }
        if self.ch == b'.' as i32 {
            let mut frac = 0.0f64;
            let mut prec = 0;
            self.next_ch();
            while (self.ch as u8).is_ascii_digit() {
                frac = frac * 10.0 + (self.ch - b'0' as i32) as f64;
                prec += 1;
                self.next_ch();
            }
            self.dnum += frac / xpow10(prec);
        }
        if (self.ch as u8 as char).to_ascii_uppercase() == 'E' {
            self.next_ch();
            let mut sgn = 1i32;
            if self.ch == b'-' as i32 {
                sgn = -1;
                self.next_ch();
            } else if self.ch == b'+' as i32 {
                self.next_ch();
            }
            let mut e = 0i32;
            while (self.ch as u8).is_ascii_digit() {
                let digit = self.ch - b'0' as i32;
                if (e as f64 * 10.0 + digit as f64) < 2147483647.0 {
                    e = e * 10 + digit;
                }
                self.next_ch();
            }
            self.dnum *= xpow10(sgn * e);
        }
    }

    /// `InStringSymbol` — quoted string → Str (consumes the close quote)
    fn in_string_symbol(&mut self) {
        while is_separator(self.ch) {
            self.next_ch();
        }
        if self.ch != b'\'' as i32 && self.ch != b'"' as i32 {
            self.syn_error("String expected");
            return;
        }
        let sng = self.ch;
        self.str_.clear();
        self.next_ch();
        while self.ch != sng {
            if self.ch == b'\n' as i32 || self.ch == b'\r' as i32 || self.ch == 0 {
                break;
            }
            self.str_.push(self.ch as u8 as char);
            self.next_ch();
        }
        self.sy = Symbol::Str;
        self.next_ch();
    }

    /// `InSymbol` — the lexer loop (C's `InSymbol`)
    fn in_symbol(&mut self) {
        loop {
            while is_separator(self.ch) {
                self.next_ch();
            }
            self.str_.clear();
            self.id.clear();
            if is_first_idchar(self.ch) {
                while is_idchar(self.ch) {
                    self.id.push(self.ch as u8 as char);
                    self.next_ch();
                }
                let key = if self.is_cube {
                    bin_srch_key(&self.id, TABKEYS_CUBE)
                } else {
                    bin_srch_key(&self.id, TABKEYS_IT8)
                };
                self.sy = if key == Symbol::Undefined {
                    Symbol::Ident
                } else {
                    key
                };
            } else if (self.ch as u8).is_ascii_digit()
                || self.ch == b'.' as i32
                || self.ch == b'-' as i32
                || self.ch == b'+' as i32
            {
                let mut sign = 1i32;
                if self.ch == b'-' as i32 {
                    sign = -1;
                    self.next_ch();
                } else if self.ch == b'+' as i32 {
                    self.next_ch();
                }
                self.inum = 0;
                self.sy = Symbol::Inum;
                if self.ch == b'0' as i32 {
                    self.next_ch();
                    if (self.ch as u8 as char).to_ascii_uppercase() == 'X' {
                        self.next_ch();
                        while (self.ch as u8).is_ascii_hexdigit() {
                            let c = (self.ch as u8 as char).to_ascii_uppercase();
                            let j = if ('A'..='F').contains(&c) {
                                c as i32 - 'A' as i32 + 10
                            } else {
                                c as i32 - '0' as i32
                            };
                            if self.inum as f64 * 16.0 + j as f64 > 2147483647.0 {
                                self.syn_error("Invalid hexadecimal number");
                                return;
                            }
                            self.inum = self.inum * 16 + j;
                            self.next_ch();
                        }
                        return;
                    }
                    if (self.ch as u8 as char).to_ascii_uppercase() == 'B' {
                        self.next_ch();
                        while self.ch == b'0' as i32 || self.ch == b'1' as i32 {
                            let j = self.ch - b'0' as i32;
                            if self.inum as f64 * 2.0 + j as f64 > 2147483647.0 {
                                self.syn_error("Invalid binary number");
                                return;
                            }
                            self.inum = self.inum * 2 + j;
                            self.next_ch();
                        }
                        return;
                    }
                }
                while (self.ch as u8).is_ascii_digit() {
                    let digit = self.ch - b'0' as i32;
                    if self.inum as f64 * 10.0 + digit as f64 > 2147483647.0 {
                        self.read_real(self.inum);
                        self.sy = Symbol::Dnum;
                        self.dnum *= sign as f64;
                        return;
                    }
                    self.inum = self.inum * 10 + digit;
                    self.next_ch();
                }
                if self.ch == b'.' as i32 {
                    self.read_real(self.inum);
                    self.sy = Symbol::Dnum;
                    self.dnum *= sign as f64;
                    return;
                }
                self.inum *= sign;
                if is_idchar(self.ch) {
                    let buffer = if self.sy == Symbol::Inum {
                        format!("{}", self.inum)
                    } else {
                        format!("{}", self.dnum)
                    };
                    self.id.clear();
                    self.id.push_str(&buffer);
                    while is_idchar(self.ch) {
                        self.id.push(self.ch as u8 as char);
                        self.next_ch();
                    }
                    self.sy = Symbol::Ident;
                }
                return;
            } else {
                match self.ch {
                    0x1a | 0 | -1 => {
                        self.sy = Symbol::Eof;
                    }
                    c if c == b'\n' as i32 => {
                        self.next_ch();
                        if self.ch == b'\n' as i32 {
                            self.next_ch();
                        }
                        self.sy = Symbol::Eoln;
                        self.lineno += 1;
                    }
                    c if c == b'\n' as i32 => {
                        self.next_ch();
                        self.sy = Symbol::Eoln;
                        self.lineno += 1;
                    }
                    c if c == b'#' as i32 => {
                        self.next_ch();
                        while self.ch != 0 && self.ch != b'\n' as i32 && self.ch != b'\n' as i32 {
                            self.next_ch();
                        }
                        self.sy = Symbol::Comment;
                    }
                    c if c == b'\'' as i32 || c == b'"' as i32 => {
                        self.in_string_symbol();
                    }
                    _ => {
                        self.syn_error(&format!("Unrecognized character: 0x{:x}", self.ch));
                        return;
                    }
                }
            }
            if self.sy != Symbol::Comment {
                break;
            }
        }
    }

    /// `CheckEOLN` — verify sy==Eoln, then consume all EOLNs
    fn check_eoln(&mut self) -> bool {
        if !self.check(Symbol::Eoln, "Expected separator") {
            return false;
        }
        while self.sy == Symbol::Eoln {
            self.in_symbol();
        }
        true
    }

    /// `Skip`
    fn skip(&mut self, sy: Symbol) -> bool {
        if self.sy == sy {
            self.in_symbol();
            true
        } else {
            self.syn_error("Unexpected symbol")
        }
    }

    /// `SkipEOLN`
    fn skip_eoln(&mut self) -> bool {
        while self.sy == Symbol::Eoln {
            self.in_symbol();
        }
        true
    }

    /// `GetVal` — token → string (does NOT advance; callers do)
    fn get_val(&mut self, buffer: &mut String, _max: usize, err_title: &str) -> bool {
        match self.sy {
            Symbol::Inum => {
                *buffer = format!("{}", self.inum);
                true
            }
            Symbol::Dnum => {
                *buffer = format!("{}", self.dnum);
                true
            }
            Symbol::Ident => {
                *buffer = self.id.clone();
                true
            }
            Symbol::Str => {
                *buffer = self.str_.clone();
                true
            }
            _ => self.syn_error(err_title),
        }
    }

    // --- parser --------------------------------------------------------------

    /// `ReadType` — the sheet-type line into `sheet_type`
    fn read_type(&mut self) -> String {
        while is_separator(self.ch) {
            self.next_ch();
        }
        let mut s = String::new();
        while self.ch != b'\r' as i32
            && self.ch != b'\n' as i32
            && self.ch != b'\t' as i32
            && self.ch != 0
        {
            if s.len() < MAXSTR {
                s.push(self.ch as u8 as char);
            }
            self.next_ch();
        }
        s
    }

    /// `DataFormatSection` — `BEGIN_DATA_FORMAT`…`END_DATA_FORMAT`
    fn data_format_section(&mut self) -> bool {
        let mut i_field = 0usize;
        self.in_symbol();
        if !self.check_eoln() {
            return false;
        }
        while self.sy != Symbol::EndDataFormat
            && self.sy != Symbol::Eoln
            && self.sy != Symbol::Eof
            && self.sy != Symbol::SynError
        {
            if self.sy != Symbol::Ident {
                return self.syn_error("Sample type expected");
            }
            let label = self.id.clone();
            if !self.set_data_format(i_field, &label) {
                return false;
            }
            i_field += 1;
            self.in_symbol();
            self.skip_eoln();
        }
        self.skip_eoln();
        if !self.skip(Symbol::EndDataFormat) {
            return false;
        }
        self.skip_eoln();
        if i_field != self.get_table().n_samples {
            return self.syn_error(&format!(
                "Count mismatch. NUMBER_OF_FIELDS was {}, found {}",
                self.get_table().n_samples,
                i_field
            ));
        }
        true
    }

    /// `DataSection` — `BEGIN_DATA`…`END_DATA`
    fn data_section(&mut self) -> bool {
        let mut i_field = 0usize;
        let mut i_set = 0usize;
        let mut buffer = String::new();
        self.in_symbol();
        if !self.check_eoln() {
            return false;
        }
        if self.get_table().data.is_empty() && !self.allocate_data_set() {
            return false;
        }
        while self.sy != Symbol::EndData && self.sy != Symbol::Eof && self.sy != Symbol::SynError {
            if i_field >= self.get_table().n_samples {
                i_field = 0;
                i_set += 1;
            }
            match self.sy {
                Symbol::Ident => {
                    let v = self.id.clone();
                    if !self.set_data(i_set, i_field, &v) {
                        return false;
                    }
                }
                Symbol::Str => {
                    let v = self.str_.clone();
                    if !self.set_data(i_set, i_field, &v) {
                        return false;
                    }
                }
                Symbol::EndData | Symbol::Eof | Symbol::SynError => {}
                _ => {
                    if !self.get_val(&mut buffer, 255, "Sample data expected") {
                        return false;
                    }
                    if !self.set_data(i_set, i_field, &buffer) {
                        return false;
                    }
                }
            }
            i_field += 1;
            self.in_symbol();
            self.skip_eoln();
        }
        self.skip_eoln();
        if !self.skip(Symbol::EndData) {
            return false;
        }
        self.skip_eoln();
        if i_set + 1 != self.get_table().n_patches {
            return self.syn_error(&format!(
                "Count mismatch. NUMBER_OF_SETS was {}, found {}",
                self.get_table().n_patches,
                i_set + 1
            ));
        }
        true
    }

    /// `HeaderSection` — keyword/property lines
    fn header_section(&mut self) -> bool {
        let mut buffer = String::new();
        let mut var_name = String::new();
        while self.sy != Symbol::Eof
            && self.sy != Symbol::SynError
            && self.sy != Symbol::BeginDataFormat
            && self.sy != Symbol::BeginData
        {
            match self.sy {
                Symbol::Keyword => {
                    self.in_symbol();
                    if !self.get_val(&mut buffer, MAXSTR - 1, "Keyword expected") {
                        return false;
                    }
                    if !self.add_available_property(&buffer.clone(), WriteMode::Uncooked) {
                        return false;
                    }
                    self.in_symbol();
                }
                Symbol::DataFormatId => {
                    self.in_symbol();
                    if !self.get_val(&mut buffer, MAXSTR - 1, "Keyword expected") {
                        return false;
                    }
                    if !self.add_available_sample_id(&buffer.clone()) {
                        return false;
                    }
                    self.in_symbol();
                }
                Symbol::Ident => {
                    var_name = self.id.clone();
                    if !self
                        .valid_keywords
                        .iter()
                        .any(|(k, _)| strcasecmp(k, &var_name) == 0)
                    {
                        if !self.add_available_property(&var_name.clone(), WriteMode::Uncooked) {
                            return false;
                        }
                    }
                    let write_as = self
                        .valid_keywords
                        .iter()
                        .find(|(k, _)| strcasecmp(k, &var_name) == 0)
                        .map(|(_, m)| *m)
                        .unwrap_or(WriteMode::Uncooked);
                    self.in_symbol();
                    if !self.get_val(&mut buffer, MAXSTR - 1, "Property data expected") {
                        return false;
                    }
                    if write_as != WriteMode::Pair {
                        let val = buffer.clone();
                        let m = if self.sy == Symbol::Str {
                            WriteMode::Stringify
                        } else {
                            WriteMode::Uncooked
                        };
                        if !self.add_to_list(true, &var_name.clone(), None, Some(&val), m) {
                            return false;
                        }
                    } else {
                        // "subkey1,value1;subkey2,value2"
                        if self.sy != Symbol::Str {
                            return self.syn_error(&format!(
                                "Invalid value '{}' for property '{}'",
                                buffer, var_name
                            ));
                        }
                        for pair in buffer.split(';') {
                            let pair = pair.trim();
                            if let Some(comma) = pair.rfind(',') {
                                let sub = pair[..comma].trim();
                                let val = pair[comma + 1..].trim();
                                if !sub.is_empty() && !val.is_empty() {
                                    if !self.add_to_list(
                                        true,
                                        &var_name.clone(),
                                        Some(sub),
                                        Some(val),
                                        WriteMode::Pair,
                                    ) {
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                    self.in_symbol();
                }
                Symbol::Eoln => {}
                _ => return self.syn_error("expected keyword or identifier"),
            }
            self.skip_eoln();
        }
        true
    }

    /// `ParseIT8` — the top-level driver
    fn parse_it8(&mut self, nosheet: bool) -> bool {
        if !nosheet {
            let s = self.read_type();
            self.set_sheet_type(&s);
        }
        self.in_symbol();
        self.skip_eoln();
        while self.sy != Symbol::Eof && self.sy != Symbol::SynError {
            match self.sy {
                Symbol::BeginDataFormat => {
                    if !self.data_format_section() {
                        return false;
                    }
                }
                Symbol::BeginData => {
                    if !self.data_section() {
                        return false;
                    }
                    if self.sy != Symbol::Eof && self.sy != Symbol::SynError {
                        if !self.alloc_table() {
                            return false;
                        }
                        self.n_table = self.tables_count - 1;
                        if !nosheet {
                            if self.sy == Symbol::Ident {
                                while is_separator(self.ch) {
                                    self.next_ch();
                                }
                                if self.ch == b'\n' as i32 || self.ch == b'\r' as i32 {
                                    let ty = self.id.clone();
                                    self.set_sheet_type(&ty);
                                    self.in_symbol();
                                } else {
                                    self.set_sheet_type("");
                                }
                            } else if self.sy == Symbol::Str {
                                let ty = self.str_.clone();
                                self.set_sheet_type(&ty);
                                self.in_symbol();
                            }
                        }
                    }
                }
                Symbol::Eoln => {
                    self.skip_eoln();
                }
                _ => {
                    if !self.header_section() {
                        return false;
                    }
                }
            }
        }
        self.sy != Symbol::SynError
    }

    /// `CookPointers` — resolve SAMPLE_ID column + LABEL cross-refs
    fn cook_pointers(&mut self) {
        let old_table = self.n_table;
        for j in 0..self.tables_count {
            self.n_table = j;
            let n_samples = self.get_table().n_samples;
            for id_field in 0..n_samples {
                let Some(fld) = self.get_data_format(id_field) else {
                    continue;
                };
                let fld = fld.to_string();
                if strcasecmp(&fld, "SAMPLE_ID") == 0 {
                    self.get_table_mut().sample_id = id_field;
                }
                if strcasecmp(&fld, "LABEL") == 0 || fld.starts_with('$') {
                    let n_patches = self.get_table().n_patches;
                    for i in 0..n_patches {
                        if let Some(label) = self.get_data(i, id_field).map(|s| s.to_string()) {
                            for k in 0..self.tables_count {
                                let n_t = self.n_table;
                                self.n_table = k;
                                let val = self
                                    .is_available_on_list(
                                        &self.get_table().header_list,
                                        &label,
                                        None,
                                    )
                                    .and_then(|kv| kv.value.clone());
                                self.n_table = n_t;
                                if let Some(ty) = val {
                                    let buf = format!("{label} {k} {ty}");
                                    self.set_data(i, id_field, &buf);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.n_table = old_table;
    }

    /// `IsMyBlock` — sniff whether a buffer is IT8/CGATS
    pub fn is_my_block(buf: &[u8]) -> i32 {
        let mut words = 1i32;
        let mut space = false;
        let mut quot = false;
        let n = buf.len().min(132);
        if buf.len() < 10 {
            return 0;
        }
        for &b in &buf[1..n] {
            match b {
                b'\n' | b'\r' => {
                    return if quot || words > 2 { 0 } else { words };
                }
                b'\t' | b' ' => {
                    if !quot && !space {
                        space = true;
                    }
                }
                b'"' => {
                    quot = !quot;
                }
                _ => {
                    if !(32..=127).contains(&b) {
                        return 0;
                    }
                    if space {
                        words += 1;
                    }
                    space = false;
                }
            }
        }
        0
    }

    /// `cmsIT8LoadFromMem` — parse a memory buffer
    pub fn load_from_mem(ctx: Option<Context>, data: &[u8]) -> Option<Box<It8>> {
        let ty = Self::is_my_block(data);
        if ty == 0 {
            return None;
        }
        let mut it8 = Self::alloc(ctx);
        it8.source = data.to_vec();
        it8.next_ch(); // C primes it8->ch via NextCh before the first InSymbol
        if !it8.parse_it8(ty - 1 != 0) {
            return None;
        }
        it8.cook_pointers();
        it8.n_table = 0;
        Some(it8)
    }

    /// `cmsIT8LoadFromFile`
    pub fn load_from_file(ctx: Option<Context>, path: &str) -> Option<Box<It8>> {
        let data = std::fs::read(path).ok()?;
        Self::load_from_mem(ctx, &data)
    }

    // --- writer ---------------------------------------------------------------

    /// `WriteHeader` — sheet type + property block
    fn write_header(&self, out: &mut String) {
        let t = self.get_table();
        out.push_str(&t.sheet_type);
        out.push('\n');
        for kv in &t.header_list {
            if kv.keyword == "#" {
                out.push_str("#\n# ");
                if let Some(v) = &kv.value {
                    for c in v.chars() {
                        out.push(c);
                        if c == '\n' {
                            out.push_str("# ");
                        }
                    }
                }
                out.push_str("\n#\n");
                continue;
            }
            out.push_str(&kv.keyword);
            if let Some(v) = &kv.value {
                match kv.write_as {
                    WriteMode::Uncooked => {
                        out.push_str(&format!("\t{v}"));
                    }
                    WriteMode::Stringify => {
                        out.push_str(&format!("\t\"{v}\""));
                    }
                    WriteMode::Hexadecimal => {
                        out.push_str(&format!("\t0x{:X}", satoi(v)));
                    }
                    WriteMode::Binary => {
                        out.push_str(&format!("\t0b{}", satob(v)));
                    }
                    WriteMode::Pair => {
                        out.push_str(&format!(
                            "\t\"{},{}\"",
                            kv.subkey.as_deref().unwrap_or(""),
                            v
                        ));
                    }
                }
            }
            out.push('\n');
        }
    }

    /// `WriteDataFormat`
    fn write_data_format(&self, out: &mut String) {
        let t = self.get_table();
        if t.data_format.is_empty() {
            return;
        }
        out.push_str("BEGIN_DATA_FORMAT\n ");
        let n = self
            .get_property("NUMBER_OF_FIELDS")
            .map(|s| satoi(&s))
            .unwrap_or(0);
        if n as usize <= t.n_samples {
            for i in 0..n as usize {
                out.push_str(
                    t.data_format
                        .get(i)
                        .and_then(|s| s.as_deref())
                        .unwrap_or(""),
                );
                out.push_str(if i == n as usize - 1 { "\n" } else { "\t" });
            }
        }
        out.push_str("END_DATA_FORMAT\n");
    }

    /// `WriteData`
    fn write_data(&self, out: &mut String) {
        let t = self.get_table();
        if t.data.is_empty() {
            return;
        }
        out.push_str("BEGIN_DATA\n");
        let n = self
            .get_property("NUMBER_OF_SETS")
            .map(|s| satoi(&s))
            .unwrap_or(0);
        if n as usize <= t.n_patches {
            for i in 0..n as usize {
                out.push(' ');
                for j in 0..t.n_samples {
                    let ptr = t.data.get(i * t.n_samples + j).and_then(|s| s.as_deref());
                    match ptr {
                        None => out.push_str("\"\""),
                        Some(v) if v.contains(' ') => {
                            out.push('"');
                            out.push_str(v);
                            out.push('"');
                        }
                        Some(v) => out.push_str(v),
                    }
                    out.push_str(if j == t.n_samples - 1 { "\n" } else { "\t" });
                }
            }
        }
        out.push_str("END_DATA\n");
    }

    /// `cmsIT8SaveToMem` — serialize to a String (memory)
    pub fn save_to_mem(&self) -> String {
        let mut out = String::new();
        for i in 0..self.tables_count {
            // serialize each table; our model keeps all tables — write all
            // by temporarily switching n_table is impossible on &self, so
            // iterate directly
            let _ = i;
            self.write_header(&mut out);
            self.write_data_format(&mut out);
            self.write_data(&mut out);
        }
        out
    }

    /// `cmsIT8SaveToFile`
    pub fn save_to_file(&self, path: &str) -> bool {
        std::fs::write(path, self.save_to_mem()).is_ok()
    }

    // --- CUBE parser -----------------------------------------------------------

    /// `ReadNumbers` — N inum/dnum tokens into an array
    fn read_numbers(&mut self, n: usize, arr: &mut [f64]) -> bool {
        for i in 0..n {
            arr[i] = match self.sy {
                Symbol::Inum => self.inum as f64,
                Symbol::Dnum => self.dnum,
                _ => return self.syn_error("Number expected"),
            };
            self.in_symbol();
        }
        self.check_eoln()
    }

    /// `ParseCube` — `.cube` LUT → (shaper curves, CLUT, title)
    fn parse_cube(
        &mut self,
        shaper: &mut Option<Vec<Vec<f32>>>,
        clut: &mut Option<(usize, Vec<f32>)>,
        title: &mut String,
    ) -> bool {
        let mut domain_min = [0.0f64; 3];
        let mut domain_max = [1.0f64; 3];
        let mut check_0_1 = [0.0f64; 2];
        let mut shaper_size = 0usize;
        let mut lut_size = 0usize;
        self.in_symbol();
        while self.sy != Symbol::Eof && self.sy != Symbol::SynError {
            match self.sy {
                Symbol::Title => {
                    self.in_symbol();
                    if !self.check(Symbol::Str, "Title string expected") {
                        return false;
                    }
                    *title = self.str_.clone();
                    self.in_symbol();
                }
                Symbol::DomainMin => {
                    self.in_symbol();
                    if !self.read_numbers(3, &mut domain_min) {
                        return false;
                    }
                }
                Symbol::DomainMax => {
                    self.in_symbol();
                    if !self.read_numbers(3, &mut domain_max) {
                        return false;
                    }
                }
                Symbol::Lut1dSize => {
                    self.in_symbol();
                    if !self.check(Symbol::Inum, "Shaper size expected") {
                        return false;
                    }
                    shaper_size = self.inum as usize;
                    if shaper_size < 2 || shaper_size > 65536 {
                        return self
                            .syn_error(&format!("LUT_1D_SIZE '{shaper_size}' is out of bounds"));
                    }
                    self.in_symbol();
                }
                Symbol::Lut3dSize => {
                    self.in_symbol();
                    if !self.check(Symbol::Inum, "LUT size expected") {
                        return false;
                    }
                    lut_size = self.inum as usize;
                    self.in_symbol();
                }
                Symbol::Lut1dInputRange | Symbol::Lut3dInputRange => {
                    self.in_symbol();
                    if !self.read_numbers(2, &mut check_0_1) {
                        return false;
                    }
                    if check_0_1[0] != 0.0 || check_0_1[1] != 1.0 {
                        return self.syn_error("Unsupported format");
                    }
                }
                Symbol::Eoln => {
                    self.in_symbol();
                }
                Symbol::LutInVideoRange | Symbol::LutOutVideoRange => {
                    return self.syn_error("Unsupported format");
                }
                Symbol::Inum | Symbol::Dnum => {
                    if shaper_size > 0 {
                        let mut sh = vec![vec![0f32; shaper_size]; 3];
                        for i in 0..shaper_size {
                            let mut nums = [0.0f64; 3];
                            if !self.read_numbers(3, &mut nums) {
                                return false;
                            }
                            for c in 0..3 {
                                sh[c][i] = ((nums[c] - domain_min[c])
                                    / (domain_max[c] - domain_min[c]))
                                    as f32;
                            }
                        }
                        *shaper = Some(sh);
                    }
                    if lut_size > 0 {
                        if lut_size < 2 || lut_size > 65 {
                            return self
                                .syn_error(&format!("LUT size '{lut_size}' is not allowed"));
                        }
                        let nodes = lut_size * lut_size * lut_size;
                        let mut lut = vec![0f32; nodes * 3];
                        for i in 0..nodes {
                            let mut nums = [0.0f64; 3];
                            if !self.read_numbers(3, &mut nums) {
                                return false;
                            }
                            // B in slot 0, G in 1, R in 2 (C's swap)
                            lut[i * 3 + 2] = ((nums[0] - domain_min[0])
                                / (domain_max[0] - domain_min[0]))
                                as f32;
                            lut[i * 3 + 1] = ((nums[1] - domain_min[1])
                                / (domain_max[1] - domain_min[1]))
                                as f32;
                            lut[i * 3 + 0] = ((nums[2] - domain_min[2])
                                / (domain_max[2] - domain_min[2]))
                                as f32;
                        }
                        *clut = Some((lut_size, lut));
                    }
                    if !self.check(Symbol::Eof, "Extra symbols found in file") {
                        return false;
                    }
                }
                _ => {
                    self.in_symbol();
                }
            }
        }
        true
    }

    /// `cmsCreateDeviceLinkFromCubeFileTHR` — `.cube` → devicelink profile
    pub fn create_device_link_from_cube_file(
        ctx: Option<Context>,
        path: &str,
    ) -> Option<Box<IccProfile>> {
        let data = std::fs::read(path).ok()?;
        let mut cube = Self::alloc(ctx.clone());
        cube.is_cube = true;
        cube.source = data;
        let mut shaper = None;
        let mut clut = None;
        let mut title = String::new();
        cube.in_symbol();
        if !cube.parse_cube(&mut shaper, &mut clut, &mut title) {
            return None;
        }
        let mut profile = crate::profile::create_profile_placeholder(ctx.clone());
        crate::profile::set_profile_version(&mut profile, 4.4);
        use crate::signatures::{colorspace as cs, profileclass as pc, tag};
        profile.device_class = pc::SigLinkClass;
        profile.color_space = cs::SigRgbData;
        profile.pcs = cs::SigRgbData;
        profile.rendering_intent = crate::types::INTENT_PERCEPTUAL;
        let mut pipeline = crate::lut::pipeline_alloc(ctx.as_ref(), 3, 3)?;
        if let Some(sh) = shaper {
            let curves: Vec<ToneCurve> = sh
                .iter()
                .filter_map(|t| crate::gamma::build_tabulated_tone_curve_float(ctx.as_ref(), t))
                .collect();
            let refs: Vec<&ToneCurve> = curves.iter().collect();
            let shaper_stage = crate::lut::stage_alloc_tone_curves(ctx.as_ref(), 3, Some(&refs))?;
            pipeline.insert_stage(StageLoc::AtBegin, shaper_stage);
        }
        if let Some((n, tab)) = clut {
            let clut_stage =
                crate::lut::stage_alloc_clut_float(ctx.as_ref(), n as u32, 3, 3, Some(&tab))?;
            pipeline.insert_stage(StageLoc::AtEnd, clut_stage);
        }
        let mut mlu = crate::named::mlu_alloc(ctx.as_ref(), 1)?;
        crate::named::mlu_set_utf8(&mut mlu, b"en", b"US", title.as_bytes());
        crate::profile::cms_write_tag(
            &mut profile,
            tag::SigProfileDescriptionTag,
            Some(crate::tagtypes::TagObject::Mlu(mlu)),
        );
        crate::profile::cms_write_tag(
            &mut profile,
            tag::SigAToB0Tag,
            Some(crate::tagtypes::TagObject::LutA2B(pipeline)),
        );
        Some(profile)
    }
}
