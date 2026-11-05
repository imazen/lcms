// Wrappers around cmsINLINE functions from lcms2_internal.h so parity tests
// can drive the same compiled code the library uses (each TU gets its own
// copy of a `static inline`; this TU is that copy).
#include "lcms2_internal.h"

int              cref_quick_floor(cmsFloat64Number val)   { return _cmsQuickFloor(val); }
cmsUInt16Number  cref_quick_floor_word(cmsFloat64Number d){ return _cmsQuickFloorWord(d); }
cmsUInt16Number  cref_quick_saturate_word(cmsFloat64Number d){ return _cmsQuickSaturateWord(d); }
cmsS15Fixed16Number cref_to_fixed_domain(int a)           { return _cmsToFixedDomain(a); }
int              cref_from_fixed_domain(cmsS15Fixed16Number a){ return _cmsFromFixedDomain(a); }
