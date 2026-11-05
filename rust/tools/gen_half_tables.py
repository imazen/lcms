#!/usr/bin/env python3
"""Extract the half-float lookup tables from cmshalf.c into half_tables.rs."""
import re

SRC = "/home/lilith/work/zen/lcms/reference/lcms2/src/cmshalf.c"
text = open(SRC).read()

def table(name):
    m = re.search(r"static const cms\w+Number " + name + r"\[\d+\] = \{(.*?)\};", text, re.S)
    vals = [v.strip() for v in m.group(1).replace("\n", " ").split(",") if v.strip()]
    return vals

specs = [("Mantissa", "u32", "mantissa"), ("Offset", "u16", "offset"),
         ("Exponent", "u32", "exponent"), ("Base", "u16", "base"), ("Shift", "u8", "shift")]

out = ["//! Half-float lookup tables, extracted verbatim from",
       "//! `reference/lcms2/src/cmshalf.c` by `rust/tools/gen_half_tables.py`.",
       "//! Exhaustively gated: the whole u16 domain is compared against C.",
       ""]
for cname, ty, rname in specs:
    vals = table(cname)
    out.append(f"pub const {rname.upper()}: [{ty}; {len(vals)}] = [")
    for i in range(0, len(vals), 8):
        out.append("    " + ", ".join(vals[i:i+8]) + ",")
    out.append("];\n")
print("\n".join(out))
