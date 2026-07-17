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
- Remediation RED/GREEN: fixture packages now use the deterministic normalized
  app archive/MSI/AppImage names and cargo-packager 0.11.8's actual single
  base64 `.sig` envelope. The parser decodes that envelope exactly once,
  preserves raw `.minisig` support, and rejects double encoding and multiple
  sidecars.

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
- The Task 10 workflow uploads the verified B-stage as
  `ffmpeg-<target-id>-bundle`; release resolution binds the artifact run to
  the exact checkout SHA and waits for a successful producer rather than using
  an unrelated latest run.

## Native-release boundary

This machine has no validated Task 10 native bundle artifact, protected
codesigning/notarization/Authenticode credentials, or protected update signing
key. Therefore it makes no claim that a signed installer, notarized DMG, MSI,
AppImage, or `.deb` passed locally. The release workflow fails closed if the
specified validated bundle artifact or protected signing inputs are absent;
PR jobs receive no signing secrets. A local macOS cargo-packager invocation
was started only to force the exact config through the real packager/build
path; full native packaging remains gated on the validated bundle.

Protected native jobs fail early on absent signing input, code-sign/notarize
and staple macOS artifacts before creating the updater archive, Authenticode
and verify the MSI before its updater signature, and sign Linux AppImage/deb.
The protected tag aggregation accepts only those normalized signed artifacts,
then produces/verifies/publishes `latest.json` and `downloads.json`.
