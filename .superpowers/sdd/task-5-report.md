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

## Review-fix GREEN

Hardware quarantine is now process-wide per backend, with an isolated constructor reserved for
deterministic policy-category tests. A new policy after quarantine starts directly on CPU with the
stable `hardware_quarantined` code, and `actual_backend` remains empty until an attempt succeeds.
Inventory now uses an exact no-common-argument path with a strict 64 KiB stdout cap and draining.
Hardware execution preflights the complete inventory and maps collection/component failure to
`probe_failed`. Forced fallback adds a backend device sentinel and an unknown FFmpeg option so a
driver that ignores device selection still fails closed before CPU is permitted.

## Second review-fix GREEN

Ordinary self-test now checks the policy's scheduled backend before spawning and rejects a
quarantined backend rather than silently using CPU. Forced fallback likewise requires a hardware
attempt, then uses a separate no-preflight forced attempt so the deliberate invalid command is
actually spawned. Normal attempts retain preflight. The hardware command builder was simplified
so VAAPI emits one device pair only, and normal runner stdout is capped while excess is drained.

## Final orchestration coverage

Unix binary integration tests now launch the compiled CLI with isolated fake FFmpeg/ffprobe
executables for every backend. They prove the forced command (including the canonical invalid
device and fail-closed option) runs and fails before the CPU VP9/Opus command, validate the two
exact output lines and evidence fields, and prove a surprising hardware success writes no pass
evidence and never starts CPU.
