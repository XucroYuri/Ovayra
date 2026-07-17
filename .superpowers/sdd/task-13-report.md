# Task 13 gate remediation report

## Implemented

- File-backed schema-v2 gate E2E builds the complete canonical matrix from
  typed `PhaseZeroProof` constructors and production serialization, then runs
  the CLI gate and verifies a deterministic PASS report with rows, required
  components, and opaque source hashes only.
- File-backed CLI rejection coverage includes missing, duplicate, unmatched,
  schema-v1, and unknown-field records. Existing linter and gate suites cover
  sensitive values, symlinks, bounded traversal, duplicate JSON keys, TOCTOU
  reads, malformed paths, and deterministic NO-GO reports.
- Task 10 uploads deterministic `ffmpeg-repro-<target>.json` v2 proofs; Task
  11 uploads deterministic `package-<target>.json` and `update-<target>.json`
  v2 proofs. Task 12 no longer writes generic FFmpeg evidence into its gate
  evidence directory.

## Protected external evidence still required

- Six real-device preview, hardware media, keyring, tray, process, and
  encrypted checkpoint runs.
- Credentialed Gemini upload/resume/ACTIVE-analysis/cleanup runs.
- Protected FFmpeg producer, native signing/notarization/Authenticode/MiniSign
  executions, and updater publication checks.

These are intentionally not synthesized into `docs/phase-0/evidence`; the
checked-in feasibility report remains NO-GO until protected runs are collected.
