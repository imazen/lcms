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

// --- expose transform internals for the prelin8/resample divergence bisect ---
#include "lcms2_internal.h"
void* cref_xform_lut(cmsHTRANSFORM t) { return ((struct _cmstransform_struct*) t)->Lut; }
void* cref_pipeline_first_stage(void* lut) { return ((cmsPipeline*) lut)->Elements; }
void* cref_stage_data(void* mpe) { return ((cmsStage*) mpe)->Data; }
// CLutData -> Tab.T (u16 table ptr) — lcms2 _cmsStageCLutData{ *Tab; union{u16*T;f32*TFloat;} }
void* cref_clut_u16_table(void* clutdata) {
    _cmsStageCLutData* d = (_cmsStageCLutData*) clutdata;
    return (void*) d -> Tab . T;
}
cmsUInt32Number cref_clut_nodes(void* clutdata) {
    _cmsStageCLutData* d = (_cmsStageCLutData*) clutdata;
    return d -> nEntries;
}
// walk stages
void* cref_stage_next(void* mpe) { return ((cmsStage*) mpe)->Next; }
cmsUInt32Number cref_stage_type(void* mpe) { return ((cmsStage*) mpe)->Type; }

// p8 = Lut->PrivateData (Prelin8Data) — X0/Y0/Z0/rx/ry/rz are u16[256] each
void* cref_lut_private_data(void* lut) { return ((cmsPipeline*) lut)->Data; }
// Prelin8Data: ContextID(8) + p(8) = 16, then rx/ry/rz u16[256]*3 = 1536, then X0/Y0/Z0 u32[256]*3
cmsUInt16Number* cref_p8_r(void* p8, int which) {   // which 0..2 = rx,ry,rz
    return (cmsUInt16Number*)((char*) p8 + 16) + which * 256;
}
cmsUInt32Number* cref_p8_x(void* p8, int which) {   // which 0..2 = X0,Y0,Z0
    return (cmsUInt32Number*)((char*) p8 + 16 + 3 * 256 * sizeof(cmsUInt16Number)) + which * 256;
}
/* cam02/sigmoid debug — dump C's inverse_sigmoid intermediates */
#include <math.h>
#include <stdint.h>
static double sb_(double k, double t){ return (1.0/(1.0+exp(-k*t)))-0.5; }
static double isb_(double k, double t){ return -log((1.0/(t+0.5))-1.0)/k; }
uint64_t cref_sigmoid_correction(double k){
    double corr = 0.5/sb_(k,1.0);
    return ((union{double d; uint64_t u;}){corr}).u;
}
uint64_t cref_sigmoid_base_bits(double k,double t){
    return ((union{double d; uint64_t u;}){sb_(k,t)}).u;
}
uint64_t cref_inverse_sigmoid_x_bits(double k,double t){
    double corr = 0.5/sb_(k,1.0);
    double x = (t-0.5)/corr;
    return ((union{double d; uint64_t u;}){x}).u;
}
uint64_t cref_inverse_sigmoid_inner_bits(double k,double t){
    double corr = 0.5/sb_(k,1.0);
    double x = (t-0.5)/corr;
    double inner = (1.0/(x+0.5))-1.0;
    return ((union{double d; uint64_t u;}){inner}).u;
}
uint64_t cref_isb_bits(double k,double t){
    return ((union{double d; uint64_t u;}){isb_(k,t)}).u;
}
uint64_t cref_inverse_sigmoid_bits(double k,double t){
    double corr = 0.5/sb_(k,1.0);
    double r = (isb_(k,(t-0.5)/corr)+1.0)/2.0;
    return ((union{double d; uint64_t u;}){r}).u;
}
#include "../include/lcms2.h"
/* read Seg->Params / Evals for a parametric curve */
double cref_seg_param(void* curve, int seg, int p){
    cmsToneCurve* g = (cmsToneCurve*)curve;
    if (!g || seg<0 || seg>=(int)g->nSegments) return -1e30;
    return g->Segments[seg].Params[p];
}
int cref_seg_type(void* curve, int seg){
    cmsToneCurve* g=(cmsToneCurve*)curve;
    return g->Segments[seg].Type;
}
uint32_t cref_seg_ngrid(void* curve, int seg){
    cmsToneCurve* g=(cmsToneCurve*)curve;
    return g->Segments[seg].nGridPoints;
}
