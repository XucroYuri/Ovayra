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

Cargo printed pre-existing workspace warnings about the pinned `toml = "=1.1.3+spec-1.1.0"`
metadata in other crate manifests; dependency pins were intentionally not modified.

## Self-review

- Confirmed that the runner does not use a linked FFmpeg library and does not retain command
  arguments or paths in its evidence type.
- Confirmed stderr is actively drained after its retained capacity is reached, preventing the
  child from blocking on a full stderr pipe.
- Confirmed negative availability tests cover missing components, nonzero exits, zero observed
  frames, and partial inventory names.
