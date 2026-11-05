// Wrappers around cmsINLINE functions from lcms2_internal.h so parity tests
// can drive the same compiled code the library uses (each TU gets its own
// copy of a `static inline`; this TU is that copy).
#include "lcms2_internal.h"

int              cref_quick_floor(cmsFloat64Number val)   { return _cmsQuickFloor(val); }
cmsUInt16Number  cref_quick_floor_word(cmsFloat64Number d){ return _cmsQuickFloorWord(d); }
cmsUInt16Number  cref_quick_saturate_word(cmsFloat64Number d){ return _cmsQuickSaturateWord(d); }
cmsS15Fixed16Number cref_to_fixed_domain(int a)           { return _cmsToFixedDomain(a); }
int              cref_from_fixed_domain(cmsS15Fixed16Number a){ return _cmsFromFixedDomain(a); }

// --- cmspack parity: formatters only read info->InputFormat/OutputFormat ---
typedef struct { cmsUInt32Number InputFormat, OutputFormat; } OrcInfo;
typedef cmsUInt8Number* (*Fmt16Fn)(OrcInfo*, cmsUInt16Number*, cmsUInt8Number*, cmsUInt32Number);
typedef cmsUInt8Number* (*FmtFlFn)(OrcInfo*, cmsFloat32Number*, cmsUInt8Number*, cmsUInt32Number);

cmsUInt8Number* orc_call_fmt16(void* fmt, OrcInfo* info, cmsUInt16Number* w, cmsUInt8Number* buf, cmsUInt32Number stride) {
    return ((Fmt16Fn) fmt)(info, w, buf, stride);
}
cmsUInt8Number* orc_call_fmtfl(void* fmt, OrcInfo* info, cmsFloat32Number* w, cmsUInt8Number* buf, cmsUInt32Number stride) {
    return ((FmtFlFn) fmt)(info, w, buf, stride);
}
void* orc_get_input_fmt16(cmsUInt32Number ty) { return (void*) _cmsGetFormatter(0, ty, cmsFormatterInput, CMS_PACK_FLAGS_16BITS).Fmt16; }
void* orc_get_input_fmtfl(cmsUInt32Number ty) { return (void*) _cmsGetFormatter(0, ty, cmsFormatterInput, CMS_PACK_FLAGS_FLOAT).FmtFloat; }
void* orc_get_output_fmt16(cmsUInt32Number ty) { return (void*) _cmsGetFormatter(0, ty, cmsFormatterOutput, CMS_PACK_FLAGS_16BITS).Fmt16; }
void* orc_get_output_fmtfl(cmsUInt32Number ty) { return (void*) _cmsGetFormatter(0, ty, cmsFormatterOutput, CMS_PACK_FLAGS_FLOAT).FmtFloat; }
