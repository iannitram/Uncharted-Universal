# Issue 299: FFmpeg distribution audit

Audit base `origin/main` 69672033bf92d102573862a66ac800082ca59a77. This is a build and distribution audit, not a legal determination.

## Existing work and remaining gaps

- `gpu/CMakeLists.txt:180,269` requires and links libavformat, libavcodec, libavutil, libswscale and libswresample. `gpu/shadps4/core/libraries/videodec/videodec2_impl.cpp` and `avplayer/avplayer_source.cpp` still use them. The Rust AJM port (e49fc984, #368) removes its own libavcodec use, not the GPU library's dependency.
- `.github/workflows/build.yml:241` and `check.yml:148` install CLANG64 MSYS2 FFmpeg. `packaging/windows/package.sh:30-33` copies CLANG64 dependencies discovered with `ldd`; there is no FFmpeg configuration or licence compatibility gate. The existing 2026-10-16 research describes a GPL-3 configuration; the actual release binary and current MSYS2 version were not inspected in this audit. The first acceptance item is not established, and the research note still says selection is open (`Research/legal-licensing-distribution.md:586-594`).
- #302 commits 1cdcfb91, 93b51d0a and 4acec55d added `packaging/licence_check.py` and Windows packaging integration (`package.sh:62-72`): discovered DLLs and selected static archives must have an owner and at least one package licence file, which is copied. `tests/test_licence_check.py` verifies the rule with synthetic inputs. This checks existence, not compatibility, completeness of all component notices, or source availability. DLLs added outside the ldd list (e.g. optional bridges) are handled by separate copy logic, not this gate. The literal every-shipped-DLL acceptance needs a manifest/coverage audit.
- `packaging/rust_licences.py` and `tests/test_rust_licences.py` cover Rust crate notices, not DLL compatibility. Both synthetic Python suites passed here (11 and 5 tests).
- Release creation in `.github/workflows/build.yml:290-324` writes a generic note and no explicit FFmpeg or other third-party source offer. Source availability and the appropriate offer for the actual chosen configuration are unverified.

## Proposed bounded approach (not implemented)

1. Select and document a reproducible, pinned Windows FFmpeg configuration with GPL-2.0-compatible components, built as shared libraries for the actual decoder/demuxer needs. Replace MSYS2 FFmpeg in the **release** build path and ensure pkg-config, linker, and staged DLLs resolve to this build. Avoid asserting that disabling `--enable-version3` alone proves the whole dependency closure compatible.
2. Fail closed at package time against the **actual staged FFmpeg binaries** and their recorded configure/license/provenance, including dependency closure and version consistency; prohibit unexpected MSYS2 FFmpeg replacements. Test rejection of incompatible or missing metadata and mismatched DLLs, plus a positive package with the chosen build. A runtime configuration probe can supplement, not replace, provenance and dependency review.
3. Inventory every staged third-party DLL including separately copied optional libraries and verify associated notices; test a deliberately missing notice fails packaging. Document exact source, configuration, licence texts and distribution/source-offer mechanism required for the actual selected dependencies in the research note and release notes. Confirm compliance before release.
4. Exercise the Windows package/CI and inspect its actual staged DLLs, notices, source material/offer and linkage; validate necessary media decode separately without game files where a synthetic fixture suffices. No claim of game playback from unit or packaging checks.

No production changes made in this audit. No upstream communication or installs. This issue remains open.
