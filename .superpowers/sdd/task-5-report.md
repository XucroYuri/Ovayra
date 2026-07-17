# Task 5 hardware fallback report

Base commit: `3496c5a`.

## RED

The required policy test was written first and failed as intended: unresolved imports for
`spike_media::AttemptOutcome` and `spike_media::ExecutionPolicy`.

Additional RED contracts then failed for the stable downgrade code, bounded two-attempt state
machine, CPU input-transcode plan, generic ffprobe validation, hardware CLI forms, and forced
invalid-device injection. The final invalid-device contract initially failed because the
non-VAAPI plans did not receive the device; it now applies `-hwaccel_device` to every non-VAAPI
hardware plan and `-vaapi_device` to VAAPI.

## GREEN

`ExecutionPolicy` permits exactly one requested hardware attempt and one CPU attempt. Hardware
probe, spawn, timeout, nonzero, missing-frame, and invalid-ffprobe outcomes quarantine hardware
for the process and schedule CPU; CPU failure is terminal. Evidence records only stable
`requested_backend`, `actual_backend`, and `downgrade_code` values. The optional compatibility
diagnostic is bounded and is not emitted as evidence.

The `inventory` command executes the exact six FFmpeg inventory commands. `self-test` runs the
selected ten-second plan without CPU fallback, requires successful exit, observed frames, and
ffprobe validation, and prints the actual hardware backend. `forced-fallback` injects a definitely
invalid device, refuses to proceed if hardware succeeds, transcodes the supplied Phase 0 synthetic
input to CPU VP9/Opus WebM, validates it through ffprobe, writes atomic schema-v1 evidence, and
prints `ACTUAL_BACKEND=cpu` and `DOWNGRADE_OBSERVED=true`.

Validation passed: `cargo fmt --all -- --check`; `cargo clippy -p spike-media -p ovayra-spike
--all-targets --all-features -- -D warnings`; `cargo test -p spike-media`; `cargo test -p
ovayra-spike`; and `git diff --check`.

## Live limitation

Pinned `OVAYRA_FFMPEG` and `OVAYRA_FFPROBE` executables are not available in this worktree, so no
real-device self-test or forced-fallback command was run. The ignored bundle-dependent CPU test
remains unrun; deterministic policy, CLI, fake-child, and evidence contracts are green.
