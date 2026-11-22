# Changelog

## [Unreleased]

### QUEUED BREAKING CHANGES

### Added

- Add opt-in ICC.1 SpecExact profiles, transforms, and curves. Lcms2Exact remains the compatibility default. (e3d3bf31b6)

- Add opt-in SpecDefault with compact ICC v2 description recovery and import diagnostics. (efd85b9542)

- Preserve unreadable rendering-independent metadata at SpecDefault import and report affected tags. (1dfb775843)

### Changed

- Declare Rust 1.89 as the minimum supported version for the workspace. (171eeebcc9)
- Use registry releases of Magetypes and Archmage 0.9.33. (25e6dedb86)

### Fixed

- Correct native date storage and planar stride buffer sizing in the C oracle tests. (929c4c0745)
- Honor ICC LUT precedence and direct absolute PCS, preserve legacy LUT encodings on resave, and validate SpecExact export models. (732a9f3c12)
