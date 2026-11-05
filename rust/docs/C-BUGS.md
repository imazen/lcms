# Confirmed upstream lcms2 bugs / edge behaviors the port reproduces on purpose

Each entry: confirmed by the C oracle build (not speculation), cite file:line.
A C bug is still the oracle — the port reproduces it byte-for-byte.

- `_cmsWriteWCharArray` `#if WCHAR_MAX > 0xFFFF` compiles FALSE — lcms only
  includes <limits.h>; WCHAR_MAX lives in <stdint.h>/<wchar.h>. Net effect:
  wchar→u16 is unconditional truncation (0x1F91E→0xF91E, -62→0xFFC2),
  NOT the '?' replacement the #if branch implies. cmstypes.c:WriteWCharArray.
- `Type_MLU_Read` does not bounds-check `MLU->MaxWchar` vs allocated —
  documented upstream; port mirrors the read-then-validate flow.
- `cmsSigSegmentedCurve` is 'curf' — the MPE curve-set element tag, easy
  to misread as 'segf'.
- `gray unrollers` broadcast wIn[0..3] — Unroll1Byte/1Word write all three
  channels from one input byte; a *documented* quirk (gray→PCS uses XYZ).
- `_cmsReasonableGridpointsByColorspace` FORCE_CLUT/CurvePts clamping has
  off-by-one rounding on LOWRESPRECALC — port mirrors the same integer math.
- `cmsGetPostScriptCRD` on a non-Lab named-color profile always fails:
  `WriteNamedColorCRD` builds a 1-profile transform; `_cmsLinkProfiles`
  for n=1 named sets `ColorSpaceOut = PCS` (Lab=3ch) but the devicelink
  LUT outputs the device colorants (CMYK=4ch); the final check
  `cmsChannelsOfColorSpace(ColorSpaceOut) != PipelineOutputChannels`
  fails -> "Couldn't link the profiles". Reproduced on oracle with a
  CMYK named profile; port keeps the same check (cmscnvrt.c ~:651 /
  cnvrt.rs link tail). Only Lab-channel named profiles could pass.
- `_cmsIOPrintf` globally rewrites ','->'.' in emitted PS text
  (cmsplugin.c locale guard) — affects every numeric + string emit;
  port mirrors it in ps2::PsWriter::out.
