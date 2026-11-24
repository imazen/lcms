# Changelog

## [Unreleased]

### QUEUED BREAKING CHANGES

### Added

- Add explicit sampled tone curves for eligible packed float matrix-shaper plans. (61129f85)

- Add opt-in ICC.1 SpecExact profiles, transforms, and curves. Lcms2Exact remains the compatibility default. (e3d3bf31b6)

- Add opt-in SpecDefault with compact ICC v2 description recovery and import diagnostics. (efd85b9542)

- Preserve unreadable rendering-independent metadata at SpecDefault import and report affected tags. (1dfb775843)

### Changed

- Pack exact byte output tables and copy alpha in the main RGB8 matrix-shaper loop. (61129f85)

- Declare Rust 1.89 as the minimum supported version for the workspace. (171eeebcc9)
- Use registry releases of Magetypes and Archmage 0.9.33. (25e6dedb86)
- Reduce linked transform code by specializing three integer formatter pairs and selecting portable integer-loop SIMD tiers. (a74fa4a4e8)

### Fixed

- Supply all variadic output pointers to the C stage retrieval gate and check real RGB and CMYK profiles with nonuniform input. (a64c59ab)

- Prepare the inverse type 4 cutoff once per curve while preserving mutable Little CMS evaluators. (9570acf721)
- Precompute exact wrapping matrix-shaper products for packed RGB8 plans. (42ed35fc4b)
- Speed up packed matrix-shaper and CLUT transforms and correct the final RGB8 CLUT corner lookup. (56bc4f4c)
- Preserve RGBA float alpha, CMYK float ink units, and gray output formatting in fused transforms. (0596a24ea4)
- Correct native date storage and planar stride buffer sizing in the C oracle tests. (929c4c0745)
- Honor ICC LUT precedence and direct absolute PCS, preserve legacy LUT encodings on resave, and validate SpecExact export models. (732a9f3c12)
