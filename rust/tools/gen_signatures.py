#!/usr/bin/env python3
"""Generate src/signatures.rs from lcms2.h. Run once per rebase:
   python3 rust/tools/gen_signatures.py > rust/crates/lcms/src/signatures.rs"""
import re

HDR = "/home/lilith/work/zen/lcms/reference/lcms2/include/lcms2.h"
text = open(HDR).read()

out = [
    "//! Signature constants, generated from `reference/lcms2/include/lcms2.h`",
    "//! by `rust/tools/gen_signatures.py`. Do not edit by hand; regenerate.",
    "//!",
    "//! C declares these as `typedef enum` / `#define`; here they are `u32`",
    "//! constants in `pub mod`s named after each C enum.",
    "",
    "#![allow(non_upper_case_globals)]",
    "#![allow(missing_docs)]",
    "#![allow(clippy::identity_op)]",
    "",
    "use crate::types::Signature;",
    "",
]

# ---- enums ----
for body, name in re.findall(r"typedef enum \{(.*?)\}\s*(cms\w+)\s*;", text, re.S):
    mod = name
    for strip in ("cms", "Signature"):
        mod = mod.replace(strip, "")
    mod = mod.lower()
    out.append(f"pub mod {mod} {{")
    out.append("    use super::Signature;")
    next_val = 0
    for line in body.splitlines():
        raw = line
        line = line.strip().rstrip(",")
        if not line or line.startswith("//"):
            c = re.search(r"//(.*)", raw)
            if c:
                out.append(f"    //{c.group(1)}")
            continue
        m = re.match(r"(cms\w+|[A-Za-z_]\w*)\s*(?:=\s*(0x[0-9A-Fa-f]+|\d+))?", line)
        if not m:
            continue
        cname, val = m.group(1), m.group(2)
        comment = re.search(r"//(.*)", raw)
        cm = f"  //{comment.group(1)}" if comment else ""
        if val is None:
            val = str(next_val)
        else:
            next_val = int(val, 0)
        rust_const = re.sub(r"^cms", "", cname)
        out.append(f"    pub const {rust_const}: Signature = {val};{cm}")
        next_val += 1
    out.append("}")
    out.append("")

# ---- loose #define cmsSig* constants ----
out.append("pub mod misc {")
out.append("    use super::Signature;")
for m in re.finditer(r"#define\s+(cmsSig\w+)\s+(0x[0-9A-Fa-f]+)", text):
    cname, val = m.group(1), m.group(2)
    rust_const = re.sub(r"^cms", "", cname)
    line_rest = text[m.end():text.index("\n", m.end())]
    c = re.search(r"//(.*)", line_rest)
    cm = f"  //{c.group(1)}" if c else ""
    out.append(f"    pub const {rust_const}: Signature = {val};{cm}")
out.append("}")

print("\n".join(out))
