# Task 3 — FFmpeg Capability and Progress Contracts

## RED evidence

1. Added `crates/spike-media/tests/progress_contract.rs` before implementation.
   - Command: `cargo test -p spike-media --test progress_contract`
   - Result: failed as expected with `E0432` because `ProgressError`, `ProgressEvent`, and
     `ProgressParser` did not exist in `spike_media`.
2. Added `crates/spike-media/tests/hardware_plans.rs` before implementation.
   - Command: `cargo test -p spike-media --test hardware_plans`
   - Result: failed as expected with `E0432` because `Backend`, `HardwarePlan`, and
     `Inventory` did not exist in `spike_media`.

## GREEN implementation

- `ProgressParser` handles incremental newline-delimited progress blocks, arbitrary byte and
  UTF-8 splits, known numeric validation, unknown-key rejection-free handling, and both a
  64 KiB pending-input limit and a 64 KiB current-block limit.
- `FfmpegRunner` invokes only an `FFmpeg` child process, prepends the exact common arguments,
  isolates stdout progress, drains stderr while retaining at most 1 MiB, and retains only exit
  code plus SHA-256 of redacted stderr in evidence.
- Hardware plans cover VideoToolbox, D3D11VA/MF, NVDEC/NVENC, and VAAPI; every plan uses the
  shared generated 10-second H.264/AAC source. VAAPI declares `/dev/dri/renderD128` explicitly.
  Availability requires exact inventory components, a zero exit, and at least one observed frame.

## Verification evidence

All commands were run in the Phase 0 worktree:

```text
cargo fmt
cargo clippy -p spike-media --all-targets --all-features -- -D warnings
  PASS

cargo test -p spike-media --all-targets --all-features
  PASS: 14 contract tests passed

git diff --check
  PASS
```

## Final review corrections (third TDD cycle)

### RED evidence

- Added a diagnostic-redaction test containing bare filenames, quoted URLs with query strings,
  Windows paths, and home-relative paths. It failed because bare names and URLs survived the old
  path-only redaction.
- Added a UTF-8 boundary inventory test. The new `byte_len` assertion initially did not compile,
  documenting the required byte-observable API.

### GREEN corrections and verification

- Stderr normalization now retains only a stable per-line diagnostic marker before SHA-256. No
  filename, relative/absolute/home path, Windows path, URL, or query value remains in hashed
  bytes; diagnostics that differ only by private names normalize identically.
- Inventory truncation now chooses the largest valid UTF-8 boundary at or below 65,536 bytes.
- Unix-host runner integration tests execute a temporary child script to prove common argument
  ordering, stdout/stderr separation, >1 MiB stderr draining, the exact six inventory calls, and
  rejection of a nonzero inventory command.

```text
cargo fmt
cargo clippy -p spike-media --all-targets --all-features -- -D warnings
  PASS
cargo test -p spike-media --all-targets --all-features
  PASS: 25 tests passed
git diff --check
  PASS
```

Cargo printed pre-existing workspace warnings about the pinned `toml = "=1.1.3+spec-1.1.0"`
metadata in other crate manifests; dependency pins were intentionally not modified.

## Self-review

- Confirmed that the runner does not use a linked FFmpeg library and does not retain command
  arguments or paths in its evidence type.
- Confirmed stderr is actively drained after its retained capacity is reached, preventing the
  child from blocking on a full stderr pipe.
- Confirmed negative availability tests cover missing components, nonzero exits, zero observed
  frames, and partial inventory names.

## Review corrections (second TDD cycle)

### RED evidence

1. `accepts_a_large_chunk_when_it_contains_many_complete_blocks` initially failed with
   `PendingInputTooLarge`, proving the old parser incorrectly capped an entire input chunk.
2. `recovers_after_each_malformed_block_error` initially failed after a malformed numeric value,
   proving parser state was retained after an error.
3. New inventory-contract tests initially failed to compile because the six-command inventory API
   (`InventoryCommand`, `InventoryOutput`, and `Inventory::from_command_outputs`) did not exist.

### GREEN corrections

- Added the exact inventory command enum: `-version`, `-buildconf`, `-hwaccels`, `-decoders`,
  `-encoders`, and `-filters`. `FfmpegRunner::collect_inventory` executes each exactly once;
  inventory construction rejects absent, duplicate, or nonzero results and bounds each retained
  output to 64 KiB.
- Reworked `ProgressParser::push` to process complete lines as it receives them, rather than
  rejecting a large total chunk. It now resets pending/current state after every malformed number,
  invalid marker, pending-line limit, and block-limit error.
- The runner now uses `kill_on_drop(true)` and explicitly kill/waits on post-spawn missing-pipe
  and reader-failure paths while returning the originating error. It drains stderr beyond the
  retained 1 MiB cap.
- Added runner unit coverage for common-argument order, stdout/stderr separation, >1 MiB stderr
  draining, and redaction before hashing.

### Correction verification

```text
cargo fmt
cargo clippy -p spike-media --all-targets --all-features -- -D warnings
  PASS

cargo test -p spike-media --all-targets --all-features
  PASS: 21 tests passed

git diff --check
  PASS
```
