# ADR 0005: FFmpeg source-correspondent LGPL-only distribution

## Status

Proposed; acceptance requires the protected `phase-0-ffmpeg` workflow on every
native target and real-device evidence where hardware APIs are claimed.

## Decision

Distribute only FFmpeg 8.1.2 built from the source and identities pinned in
`packaging/ffmpeg.lock`. Each bundle includes the FFmpeg release tarball and
signature, libvpx and Opus source archives, license texts, exact build
configuration, local changes, SHA-256 manifest, NOTICE, and CycloneDX SBOM.
The validator rejects GPL/nonfree configuration and source-correspondence gaps.

The Windows chain is MSVC-only: PowerShell resolves `VsDevCmd` through `vswhere`,
requires `cl.exe`, `link.exe`, and `lib.exe`, and starts MSYS2 only as the POSIX
shell needed by upstream configure scripts. libvpx uses its VS17 target; Opus uses
CMake/Ninja with `cl`; FFmpeg configures with `--toolchain=msvc`. No MinGW
compiler or MinGW target triple is supported for this bundle.

NVENC/NVDEC support uses `nv-codec-headers` only as a hardware API interface.
It does not make an FFmpeg build GPL; the LGPL-only decision remains contingent
on the recorded configure flags and all independently applicable driver/SDK and
redistribution terms.

## Reproducibility boundary

The Task 10 workflow compares two clean native builds byte-for-byte for the
unsigned `ffmpeg`/`ffprobe` binaries and full staged provenance tree. It does
not create signed installers or archives, so no nondeterministic packaging layer
is claimed or accepted here. If a later signing or archive layer is
nondeterministic, the exact layer and normalized comparison procedure must be
recorded here before it can be accepted; a passing signed package hash is never
substituted for the unsigned binary/provenance comparison.

## Current evidence boundary

This repository enforces source/signature, immutable-lock, bundle-layout,
capability-inventory, and two-build comparison procedures in automation. It does
not yet contain a successful native double-build evidence record: local GPG,
PowerShell, and the three native FFmpeg build environments were unavailable for
this implementation pass. This ADR therefore claims no passing FFmpeg build,
capability inventory, CPU smoke, or reproducibility result; the protected native
workflow remains the acceptance gate.
