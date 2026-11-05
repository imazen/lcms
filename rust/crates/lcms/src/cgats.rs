//! `cmscgats.c` — the IT8/CGATS file format: a line-oriented text
//! format with properties, data-format descriptors, and patch data.
//! Port covers the data model, lexer, parser, writer, and the 39
//! public exports.

use crate::context::Context;
use crate::io::IoHandler;

pub const MAXSTR: usize = 1024;
pub const MAXTABLES: usize = 255;
pub const MAXINCLUDE: usize = 20;
pub const MAXID: usize = 128;
pub const MAX_PATH: usize = 256;
pub const MAXIT8PATCHES: usize = 100;

/// `SYMBOL` — the lexer token type
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symbol {
    Undefined,
    Inum,
    Dnum,
    Ident,
    String,
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

/// `WRITEMODE` — how a property value is serialized
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Uncooked,
    Stringify,
    Hexadecimal,
    Binary,
    Pair,
}

/// `KEYWORD` — a predefined IT8 keyword
#[derive(Clone, Copy)]
pub struct Keyword {
    pub id: &'static str,
    pub sy: Symbol,
}

/// `KEYVALUE` — a property entry (name/subkey + value + write mode)
#[derive(Clone)]
pub struct KeyValue {
    pub keyword: String,
    pub subkey: String,
    pub value: String,
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
    pub data_format: Vec<String>,
    pub data: Vec<String>,
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

/// `cmsIT8` — the document handle
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
    // predefined lists
    pub valid_keywords: Vec<Keyword>,
    pub valid_sample_ids: Vec<String>,
    pub double_formatter: String,
}

/// `TabKeysIT8`
const TABKEYS_IT8: &[Keyword] = &[
    Keyword { id: "$INCLUDE", sy: Symbol::Include },
    Keyword { id: ".INCLUDE", sy: Symbol::Include },
    Keyword { id: "BEGIN_DATA", sy: Symbol::BeginData },
    Keyword { id: "BEGIN_DATA_FORMAT", sy: Symbol::BeginDataFormat },
    Keyword { id: "DATA_FORMAT_IDENTIFIER", sy: Symbol::DataFormatId },
    Keyword { id: "END_DATA", sy: Symbol::EndData },
    Keyword { id: "END_DATA_FORMAT", sy: Symbol::EndDataFormat },
    Keyword { id: "KEYWORD", sy: Symbol::Keyword },
];

/// `TabKeysCUBE`
const TABKEYS_CUBE: &[Keyword] = &[
    Keyword { id: "DOMAIN_MAX", sy: Symbol::DomainMax },
    Keyword { id: "DOMAIN_MIN", sy: Symbol::DomainMin },
    Keyword { id: "LUT_1D_SIZE", sy: Symbol::Lut1dSize },
    Keyword { id: "LUT_1D_INPUT_RANGE", sy: Symbol::Lut1dInputRange },
    Keyword { id: "LUT_3D_SIZE", sy: Symbol::Lut3dSize },
    Keyword { id: "LUT_3D_INPUT_RANGE", sy: Symbol::Lut3dInputRange },
    Keyword { id: "LUT_IN_VIDEO_RANGE", sy: Symbol::LutInVideoRange },
    Keyword { id: "LUT_OUT_VIDEO_RANGE", sy: Symbol::LutOutVideoRange },
    Keyword { id: "TITLE", sy: Symbol::Title },
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
    "SAMPLE_ID", "STRING",
    "CMYK_C", "CMYK_M", "CMYK_Y", "CMYK_K",
    "D_RED", "D_GREEN", "D_BLUE", "D_VIS", "D_MAJOR_FILTER",
    "RGB_R", "RGB_G", "RGB_B",
    "SPECTRAL_NM", "SPECTRAL_PCT", "SPECTRAL_DEC",
    "XYZ_X", "XYZ_Y", "XYZ_Z",
    "XYY_X", "XYY_Y", "XYY_CAPY",
    "LAB_L", "LAB_A", "LAB_B", "LAB_C", "LAB_H",
    "LAB_DE", "LAB_DE_94", "LAB_DE_CMC", "LAB_DE_2000",
    "MEAN_DE",
    "STDEV_X", "STDEV_Y", "STDEV_Z",
    "STDEV_L", "STDEV_A", "STDEV_B", "STDEV_DE",
    "CHI_SQD_PAR",
];

// --- lexer --------------------------------------------------------------

/// `isseparator`
fn is_separator(c: i32) -> bool {
    c == b' ' as i32 || c == b'\t' as i32 || c == b'\n' as i32
}
/// `ismiddle`
fn is_middle(c: i32) -> bool {
    !is_separator(c) && c != 0 && c != b'\"' as i32 && c != b'\'' as i32
        && c != b'(' as i32 && c != b')' as i32 && c != b'[' as i32 && c != b']' as i32
}
/// `isidchar`
fn is_idchar(c: i32) -> bool {
    (c as u8 as char).is_ascii_alphanumeric() || c == b'_' as i32 || c == b'-' as i32
        || c == b'(' as i32 || c == b')' as i32
}
/// `isfirstidchar`
fn is_first_idchar(c: i32) -> bool {
    (c as u8 as char).is_ascii_alphabetic() || c == b'_' as i32
}
/// `isabsolutepath`
fn is_absolute_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    path.starts_with('/') || (path.len() > 1 && path.as_bytes()[1] == b':')
}

/// `BuildAbsolutePath` — join rel onto base's directory
fn build_absolute_path(rel: &str, base: &str, buffer: &mut String, max_len: usize) -> bool {
    if is_absolute_path(rel) {
        if rel.len() >= max_len {
            return false;
        }
        *buffer = rel.to_string();
        return true;
    }
    // find last separator in base
    let sep = base.rfind('/');
    if let Some(pos) = sep {
        let dir = &base[..pos + 1];
        if dir.len() + rel.len() >= max_len {
            return false;
        }
        *buffer = format!("{dir}{rel}");
        true
    } else {
        if rel.len() >= max_len {
            return false;
        }
        *buffer = rel.to_string();
        true
    }
}

/// `NoMeta` — strip a leading '#', trailing spaces
fn no_meta(s: &str) -> &str {
    let s = s.trim_start_matches('#');
    s.trim_end()
}

/// `xpow10`
fn xpow10(n: i32) -> f64 {
    if n > 0 {
        10f64.powi(n)
    } else {
        1.0 / 10f64.powi(-n)
    }
}

/// `BinSrchKey`
fn bin_srch_key(id: &str, keys: &[Keyword]) -> Symbol {
    for k in keys {
        if k.id == id {
            return k.sy;
        }
    }
    Symbol::Ident
}

impl It8 {
    /// `cmsIT8Alloc` — empty document with one table
    pub fn alloc(ctx: Option<Context>) -> Box<It8> {
        let mut it8 = Box::new(It8 {
            ctx,
            tables_count: 0,
            n_table: 0,
            is_cube: false,
            tables: Vec::new(),
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
            double_formatter: "%.10g".to_string(),
        });
        it8.tables.push(Table::new());
        it8.tables_count = 1;
        // seed the predefined lists
        for (id, mode) in PREDEFINED_PROPERTIES {
            it8.add_available_property(id, *mode);
        }
        for id in PREDEFINED_SAMPLE_IDS {
            it8.add_available_sample_id(id);
        }
        it8
    }

    /// `GetTable` — the current table
    pub fn get_table(&self) -> &Table {
        &self.tables[self.n_table]
    }
    /// `GetTable` — mutable
    pub fn get_table_mut(&mut self) -> &mut Table {
        &mut self.tables[self.n_table]
    }

    /// `AddAvailableProperty` — register a valid property name
    fn add_available_property(&mut self, key: &str, mode: WriteMode) {
        self.tables[0].header_list.push(KeyValue {
            keyword: key.to_string(),
            subkey: String::new(),
            value: String::new(),
            write_as: mode,
        });
    }

    /// `AddAvailableSampleID` — register a valid sample-id
    fn add_available_sample_id(&mut self, key: &str) {
        self.valid_sample_ids.push(key.to_string());
    }

    /// `cmsIT8GetSheetType`
    pub fn get_sheet_type(&self) -> &str {
        &self.tables[0].sheet_type
    }

    /// `cmsIT8SetSheetType`
    pub fn set_sheet_type(&mut self, ty: &str) -> bool {
        self.tables[0].sheet_type = ty.to_string();
        true
    }

    /// `cmsIT8SetComment`
    pub fn set_comment(&mut self, val: &str) -> bool {
        self.tables[0].header_list.push(KeyValue {
            keyword: "COMMENT".to_string(),
            subkey: String::new(),
            value: val.to_string(),
            write_as: WriteMode::Uncooked,
        });
        true
    }

    /// `cmsIT8SetTable`
    pub fn set_table(&mut self, n_table: usize) -> i32 {
        if n_table >= self.tables_count {
            // extend with empty tables up to n_table
            while self.tables.len() <= n_table {
                self.tables.push(Table::new());
            }
            self.tables_count = self.tables.len();
        }
        self.n_table = n_table;
        n_table as i32
    }

    // --- TODO(cmscgats): the lexer (NextCh/InSymbol/InStringSymbol/
    // ReadReal/ParseFloatNumber), the parser (ParseIT8Header/
    // ParseDataFormat/ParseData/ParseFile/ReadReal/ReadChar/ReadField),
    // the writer (WriteHeader/WriteDataFormat/WriteData/SaveFile/
    // SaveToMem), and the remaining 35 exports (SetPropertyStr/Dbl/
    // Hex/Uncooked/Multi, GetPropertyStr/Dbl/Multi, GetPatchID/Name,
    // FindDataFormat/SetDataFormat, GetDataDbl/SetDataDbl/GetDataDblIndex/
    // SetDataDblIndex, GetDataSetID/SetDataSetID, GetDataFormat/
    // GetDataFormatStr/GetDataSetStr/SetDataSetStr/GetDataSetId/
    // SetDataSetId/LCL2PCS/PCS2LCL/SetTableIndex/GetTableIndex/
    // GetTableData/SaveToFile/SaveToMem) — the bulk of the file.
}
