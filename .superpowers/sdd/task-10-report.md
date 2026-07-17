# Task 10 report: LGPL-only FFmpeg provenance chain

## TDD evidence

- RED: `cargo test -p spike-release --test ffmpeg_policy` failed because
  `FfmpegBundle` and `FfmpegPolicyError` were absent.
- GREEN: the fixture suite covers GPL/nonfree tokens (including quotes and
  whitespace), duplicate configure tokens, missing correspondence material,
  duplicate SHA256SUMS entries, escaping executable symlinks, and a complete
  CycloneDX/license/checksum layout.
- RED/GREEN: the release CLI contract was added before `release verify-ffmpeg`
  dispatch and then passed.

## Provenance verification

The FFmpeg official release index supplied the 8.1.2 xz tarball and detached
signature. A local SHA-256 calculation produced
`8c56e9c1a92e833d00caf16975ae6b67c571a1f6949321262ef3d425d816d2a8`.
FFmpeg's official download page identifies the release key fingerprint as
`FCF986EA15E6E293A5644F10B4322F04D67658D8`. Official upstream `git ls-remote`
resolved libvpx `v1.16.0^{}` to
`1024874c5919305883187e2953de8fcb4c3d7fa6`, Opus `v1.6.1^{}` to
`22244de5a79bd1d6d623c32e72bf1954b56235be`, and FFmpeg `n8.1.2^{}` to
`38b88335f99e76ed89ff3c93f877fdefce736c13`.

Local detached-signature verification was not run because this machine has no
`gpg`, `gpg2`, or `gpgv`. The protected workflow imports the FFmpeg-published
`ffmpeg-devel.asc` key artifact into an isolated temporary
`GNUPGHOME`, runs `gpg --status-fd` detached verification, and accepts exactly
one `VALIDSIG` record only when both signer and primary fingerprints match the
pinned lock fingerprint. It writes a deterministic, redacted attestation only
after that check; it does not trust printed human-readable GPG output.

## Native-build limitation

No complete FFmpeg build or double-build reproducibility run was claimed
locally: the required macOS/Linux/Windows toolchains and all dependency sources
were not available in this environment. The workflow performs two isolated
native builds per target, capability inventory, CPU VP9/Opus smoke, policy
validation, and byte comparison; any unavailable capability or failed check is
a job failure, not a pass.

## Review remediation

The remediation commit adds platform-marker-aware Windows executable paths,
exact first-banner identity checks, source-lock/signature-attestation matching,
SBOM archive-hash matching, adversarial replacement tests, a normalized
relative-path/hash/size reproducibility comparator, explicit capability checks,
and an MSYS2/MSVC Windows orchestration path. FFmpeg is extracted from the
verified official release tarball; a separate shallow checkout is used only to
obtain the pinned tag timestamp, never as build input. `nv-codec-headers`
n13.0.19.0 is pinned to `e844e5b26f46bb77479f063029595293aa8f812d` with its
MIT source/license provenance for Linux and Windows builds.

## Latest remediation verification

The final remediation adds portable SHA-256 selection (`sha256sum`, then
`shasum -a 256`) to the signature helper, with PATH-isolated fixtures covering
both branches; workflow path filters cover every helper and uploads the exact
A/B stage and evidence paths with `if-no-files-found: error`. Local verification
passed Bash syntax and helper fixtures, Task 10 policy tests, the application
test suite, formatting, strict Clippy, and `git diff --check`. PowerShell,
local GPG verification, and native FFmpeg double builds remain unrun locally.
