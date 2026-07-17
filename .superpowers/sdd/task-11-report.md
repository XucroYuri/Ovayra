# Task 11 report: package formats, update integrity, and signing gates

## TDD evidence

- RED: `cargo test -p spike-release --test manifest_signature` failed on the
  absent `ReleaseManifest` and `ReleaseVerifier` exports.
- GREEN: the real fixture suite now covers a strict three-target manifest,
  unknown JSON fields, equal/downgrade/prerelease versions, invalid URL/format
  pairs, one-byte payload/signature/public-key tampering, signed package
  discovery, separate `.deb` download metadata, and an isolated corruption
  rejection check that proves the source package is unchanged.
- RED: the new release CLI test failed on absent packaging/manifest variants.
  GREEN: it parses the bounded prepare/manifest/verify/tamper contracts.

## Fixture hygiene

The fixture was generated using official Minisign 0.12 with a one-time key in
an explicit `/tmp/ovayra-minisign-fixture.*` directory. It was verified with
the official client. The committed fixture contains only the public key,
payload, and detached signature; the temporary private key was explicitly
unlinked immediately after verification. A repository scan found no Minisign
private-key header outside that temporary directory.

## Local verification

- `cargo test --locked -p spike-release --test manifest_signature`
- `cargo test --locked -p ovayra-spike cli::tests`
- `cargo clippy --locked -p spike-release -p ovayra-spike --all-targets -- -D warnings`
- `cargo fmt` and `git diff --check`
- Exact `cargo-packager 0.11.8` is installed and its real CLI confirmed the
  config option and all requested package format names.

## Native-release boundary

This machine has no validated Task 10 native bundle artifact, protected
codesigning/notarization/Authenticode credentials, or protected update signing
key. Therefore it makes no claim that a signed installer, notarized DMG, MSI,
AppImage, or `.deb` passed locally. The release workflow fails closed if the
specified validated bundle artifact or protected signing inputs are absent;
PR jobs receive no signing secrets. A local macOS cargo-packager invocation
was started only to force the exact config through the real packager/build
path; full native packaging remains gated on the validated bundle.
