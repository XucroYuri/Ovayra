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
Its producer gate resolves the release tag to the checked-out commit and
requires a successful, push-triggered `phase-0-ffmpeg` run for that exact SHA.
Manual dispatch requires both that tag and the producer run ID, then performs
the identical GitHub API provenance checks before it can download an artifact.

## Phase 0 trust-anchor limitation

`packaging/update.pub` is the immutable, embedded Phase 0 verifier anchor. The
verifier pins the exact public-key bytes, SHA-256
`041bc31a4b9cf035aab3a907b3abba166e5d6f7372f69451879607903ff9d841`,
and Minisign key ID `8D505269004B7685`; a caller cannot substitute a key.
It intentionally contains public material only and currently matches the
non-production fixture key; it is not a commercial-release key. Protected
signing must provide a private key that verifies against these exact bytes or
the pinned verifier fails. Both updater and installer-download metadata bind
length, SHA-256, and a detached signature, and the corruption check exercises
each payload class. A key-rotation ADR and a separately provisioned production
key are required before any commercial release claim.

## Signing-key status and release gate

The current anchor is deliberately the copied test-fixture public key. No
matching private key is stored in this workspace, and this task has no evidence
that such a private key is available to the protected environment. Therefore
protected signing with the current anchor is intentionally *not* a release PASS:
the post-sign `verify-artifact` gate rejects any signer whose public key does
not equal the embedded anchor, before inspection/upload/publishing. Before a
commercial or production release, an authorized external key ceremony must
provision a new Phase 0 private key only in the protected environment, commit
its corresponding public anchor and reviewed SHA/key-ID, and then demonstrate
the same gates. No private key was generated or persisted locally.

## Extracted-artifact inspection

The release workflow now validates the final signed app/DMG, MSI, AppImage, and
deb through extraction rather than filename presence. It requires one resource
root and nonempty regular (not symlink) application, FFmpeg binaries, notices,
licenses, provenance sources/attestation/lock/build data/checksums, and SBOM;
NV codec material is required on the Windows/Linux targets. The DMG is mounted
read-only/no-browse with a cleanup trap, MSI uses a fresh administrative extract
with an exit-code/log gate, and AppImage/deb use native extraction. Fixture
tests cover valid, missing, and symlinked trees; producer-event fixtures cover
PR, branch, repository, workflow, SHA, and tag rejection.
