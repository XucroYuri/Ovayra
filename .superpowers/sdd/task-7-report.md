# Task 7 — Gemini resumable upload spike

Base commit: `b38ed55`

## TDD evidence

- RED: `cargo test -p spike-platform --test envelope` failed with unresolved
  `EnvelopeCipher` and `MemorySecretStore`; `cargo test -p spike-gemini --test
  resumable_contract` then failed because the Gemini client and DTOs did not exist.
- RED review follow-up: retry bounds, server-authoritative mismatch recovery, cleanup after
  terminal analysis failure, and redacted generation measurements were added through failing
  contract/application-orchestration coverage.
- GREEN: `cargo test -p spike-gemini --test resumable_contract -- --test-threads=1`
  passes 12 resumable contract tests (Wiremock and bounded local fault-server cases), including bounded chunk `429`/`5xx` retries, chunk
  `4xx` refusal, capped nonzero `Retry-After`, and redacted empty-generation metrics.
- GREEN: application orchestration tests prove a server offset supersedes the checkpoint hint
  and that a terminal analysis failure still performs remote cleanup.
- GREEN: `cargo test -p spike-platform --test envelope` passes encrypted round-trip and
  tamper-rejection tests.
- GREEN: `cargo test -p spike-gemini -p spike-platform -p ovayra-spike` passes.
- GREEN: `cargo clippy -p spike-gemini -p spike-platform -p ovayra-spike --all-targets -- -D warnings`
  passes.
- GREEN: `cargo fmt` and `git diff --check` pass.

## Controlled live-command status

Both command shapes compiled and their `--help` contracts were executed. The exact
`stage-upload` and `resume-analyze` commands were also attempted with
`OVAYRA_TARGET_ID=macos-arm64-vt`; each stopped before network/keyring use because
`target/phase-0/fallback.webm` is absent. No credentials, remote file, checkpoint,
or evidence were created. Gemini success and native-keyring success are therefore
not claimed. The required real-device matrix and a protected
`OVAYRA_GEMINI_API_KEY` remain prerequisites for the live gate.

## Other validation limitation

`cargo deny check advisories licenses bans sources` ran: advisories, bans, and
sources passed, while licenses failed because the existing deny configuration does
not allow workspace-wide standard licenses (including MIT and Apache-2.0). This
pre-existing workspace policy issue is outside Task 7's dependency pins.

## Commits

- Initial implementation: `ba24fd2180b2c838e14df0c803d1917c1d607b8c`.
- Review-remediation candidate: recorded by the enclosing atomic commit; this report does not
  claim its own self-referential final hash.

## Deterministic behavior map

| Behavior | Production-path test | Command |
| --- | --- | --- |
| Non-final chunk retries only 429/5xx, not 4xx | `chunk_retries_429_and_5xx_but_not_4xx` | `cargo test -p spike-gemini --test resumable_contract -- --test-threads=1` |
| Ambiguous chunk accepts only exact server offset | `ambiguous_chunk_transport_queries_and_accepts_exact_expected_offset` | same command |
| Ambiguous chunk lower/higher offsets fail closed | `ambiguous_chunk_lower_observed_offset_fails_without_stale_replay`; `ambiguous_chunk_higher_observed_offset_fails_without_stale_replay` | same command |
| Persistent processing timeout with bounded policy | `persistent_processing_returns_poll_timeout` | same command; test loops 20 times |
| Empty decoded generation is redacted metrics | `decoded_empty_generation_returns_redacted_failure_metrics` | same command |
| Misaligned server offset fails with redacted evidence | `resume_misaligned_offset_writes_redacted_failed_evidence` | `cargo test -p ovayra-spike -- --test-threads=1` |
| Beyond-input server offset fails with redacted evidence | `resume_beyond_input_offset_writes_redacted_failed_evidence` | same command |
| Mismatched offset continuation failure is persisted before return | `resume_continuation_failure_after_offset_mismatch_writes_failed_evidence` | same command |
| Omitted upload granularity uses 8 MiB chunks and re-queries before finalization | `omitted_granularity_uses_eight_mib_chunk_then_queries_before_finalizing` | same command |
| Failed remote deletion retains encrypted checkpoint for recovery | `remote_delete_failure_retains_checkpoint_and_writes_recovery_evidence` | same command |
| Empty generated analysis records redacted response metrics then fails | `empty_generation_writes_redacted_metrics_and_returns_failure` | same command |

Stability evidence: `resumable_contract` passed 20 serial runs with a per-run external
10-second cap; each 12-test run completed in 3.48–3.77 seconds. The local fault servers
have a two-second lifecycle bound and one-second socket I/O bounds.
