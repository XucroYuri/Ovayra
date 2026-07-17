# Task 2 report: Phase 0 evidence contracts

## Scope

Implemented the experimental `spike-contracts` evidence schema and strict Phase 0 matrix contract. No dependency pins were changed.

## RED / GREEN evidence

1. Added the three specified evidence contract tests before implementation.
2. Ran `cargo test -p spike-contracts --test evidence_contract`.
   - RED observed: compilation failed with unresolved root imports for `Evidence`, `EvidenceError`, `SpikeId`, `TargetId`, and `Verdict`, as expected.
3. Implemented the minimal evidence schema, case-insensitive forbidden-field rejection, finished-only JSON serialization, and root re-exports.
4. Re-ran the targeted test: GREEN, 3 passed.
5. Added matrix parsing, required-verdict, and checked-in coverage tests.
6. Ran the targeted test before creating the matrix file.
   - RED observed: `checked_in_matrix_covers_every_required_real_device_capability` failed with `No such file or directory` for `packaging/phase-0-matrix.toml`.
7. Implemented the TOML loader and deterministic validation, then added the 33-entry supported-target matrix.
8. Re-ran the targeted test: GREEN, 6 passed.

## Contract decisions

- Evidence schema version is fixed at `1`; measurements are ordered with `BTreeMap`.
- Every forbidden field-name substring is rejected case-insensitively: `api_key`, `token`, `secret`, `password`, `upload_url`, `prompt`, `result`, `media_path`, and `file_name`.
- Matrix parsing rejects unknown TOML fields, empty matrices/qualifiers, duplicate records, and targets outside the six supported real-device IDs.
- Required real-device evidence accepts only `pass`; `conditional`, `skipped`, and `fail` are rejected.
- The matrix covers hardware media backends and CPU fallback for each required target; session-specific preview/platform entries; Gemini on every target; and distribution evidence only for macOS, Windows, and Linux packaging targets.

## Final verification

All commands exited 0:

```text
cargo fmt --all -- --check
cargo clippy -p spike-contracts --all-targets --all-features -- -D warnings
cargo test -p spike-contracts
git diff --check
```

Cargo emitted pre-existing workspace warnings about the exact `toml = "=1.1.3+spec-1.1.0"` metadata requirement; this task did not alter dependency pins.

## Review hardening follow-up

Added focused RED tests for private/validated evidence serialization, nested measurement key redaction, guarded observations, validated target construction/deserialization, schema-version and unknown-field rejection, exact matrix coverage, malformed matrix records, and entry-bound verdict validation.

The RED command `cargo test -p spike-contracts --test evidence_contract` failed as intended because the hardened APIs and signatures were absent (`TargetId::new(...).unwrap()`, `observe`, `from_json`, `SensitiveObservation`, and entry-bound `validate_required_verdict`).

GREEN implements a private `Evidence` state plus private `deny_unknown_fields` serde document, recursive `Value` inspection (including arrays), guarded observations, exact six-target `TargetId` validation, and a matrix verdict API that rejects missing entries and every non-`Pass` result. The evidence type deliberately does not implement serde traits; a compile-fail doctest demonstrates that direct serde serialization is unavailable.

## Canonical matrix follow-up

Added RED tests for a reduced but structurally valid matrix and an invented Gemini/backend combination on a valid target. The first RED compile failed because the exact-set error variants did not exist.

The loader now validates the parsed matrix against a deterministic, static canonical set of all 33 required combinations. It returns `MissingRequiredEntries` for omissions and `UnsupportedRequiredEntries` for invented combinations, while preserving malformed-TOML, duplicate, empty-qualifier, and unsupported-target rejection. The checked-in matrix remains equal to the canonical set.
