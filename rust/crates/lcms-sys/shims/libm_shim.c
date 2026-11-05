#define _GNU_SOURCE
/* Safe wrappers that force real glibc libm calls. Rust's llvm.* math
 * intrinsics lower to compiler_builtins/fdlibm which differ from glibc
 * by ~1 ULP on some inputs; a C shim keeps the call a libm PLT call. */
#include <math.h>

double lcms_g_cos(double x)    { double s, c; sincos(x, &s, &c); return c; }
double lcms_g_sin(double x)    { double s, c; sincos(x, &s, &c); return s; }
double lcms_g_tan(double x)    { return tan(x); }
double lcms_g_atan2(double y, double x) { return atan2(y, x); }
double lcms_g_atan(double x)   { return atan(x); }
double lcms_g_asin(double x)   { return asin(x); }
double lcms_g_acos(double x)   { return acos(x); }
double lcms_g_hypot(double x, double y) { return hypot(x, y); }
double lcms_g_fmod(double x, double y)  { return fmod(x, y); }
double lcms_g_cbrt(double x)   { return cbrt(x); }
double lcms_g_log2(double x)   { return log2(x); }
double lcms_g_exp2(double x)   { return exp2(x); }
double lcms_g_expm1(double x)  { return expm1(x); }
double lcms_g_log1p(double x)  { return log1p(x); }
void   lcms_g_sincos(double x, double *s, double *c) { sincos(x, s, c); }
