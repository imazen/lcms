#define _GNU_SOURCE
/* Safe wrappers that force real system libm calls. Rust's llvm.* math
 * intrinsics lower to compiler_builtins/fdlibm which differ from the
 * system libm by ~1 ULP on some inputs; a C shim keeps the call a libm
 * PLT call.
 *
 * sin/cos pairing mirrors the C oracle's codegen per toolchain:
 *  - GCC (glibc/BSD/mingw): merges sin(x)+cos(x) -> sincos()
 *  - clang/Darwin: merges to __sincos()
 *  - MSVC: no merge at all (MSVCRT has no sincos) */
#if defined(_MSC_VER)
static void m_sincos(double x, double *s, double *c) { *s = sin(x); *c = cos(x); }
#elif defined(__APPLE__)
#define m_sincos(x, s, c) __sincos((x), (s), (c))
#else
#define m_sincos(x, s, c) sincos((x), (s), (c))
#endif

#include <math.h>

double lcms_g_cos(double x)    { double s, c; m_sincos(x, &s, &c); return c; }
double lcms_g_sin(double x)    { double s, c; m_sincos(x, &s, &c); return s; }
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
void   lcms_g_sincos(double x, double *s, double *c) { m_sincos(x, s, c); }
