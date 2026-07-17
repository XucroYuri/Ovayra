# Task 4 CPU WebM fallback report

Base commit: `e9c1ef2`.

## RED

Added the required ignored live integration test before implementing the fallback.

```text
cargo test -p spike-media --test cpu_fallback -- --ignored --exact produces_gemini_compatible_vp9_opus_webm
error[E0432]: unresolved imports `spike_media::CpuFallback`, `spike_media::FfprobeReport`
```

This is the intended RED failure: neither required type existed.

## GREEN

Implemented an FFmpeg/ffprobe-child-only CPU fallback. Its generated input is exclusively `lavfi`; the output plan explicitly maps video and audio and encodes VP9 (`libvpx-vp9`) plus Opus (`libopus`) in yuv420p WebM. `Command::output` drains both child streams before results are interpreted. Process errors carry no child paths or raw stderr.

The non-ignored contracts cover the canonical 10-second argument vector, caller-supplied duration, nonzero child rejection, malformed and incompatible ffprobe JSON, zero-byte and zero-duration rejection, progress-derived average speed, SHA-256 content hashing/redaction, explicit CLI flags, and the 10-second CLI default.

The CLI requires `OVAYRA_EVIDENCE_TARGET`; it is passed through strict `TargetId` validation instead of inferring a hardware class. Evidence is schema v1 and contains only media duration, byte count, average speed, codecs, pixel format, FFmpeg build ID, verdict, and the output SHA-256—never an output/evidence path or raw process output.

Validation passed:

```text
cargo fmt --all -- --check
cargo clippy -p spike-media -p ovayra-spike --all-targets --all-features -- -D warnings
cargo test -p spike-media
cargo test -p ovayra-spike
git diff --check
```

The pinned `OVAYRA_FFMPEG` and `OVAYRA_FFPROBE` executables are unavailable in this environment, so the live ignored test and CLI smoke command were deliberately not run. They require those pinned variables and remain ready for a target runner.
