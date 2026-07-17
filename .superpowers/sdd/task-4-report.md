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

Implemented an FFmpeg/ffprobe-child-only CPU fallback. Its generated input is exclusively `lavfi`; the output plan explicitly maps video and audio and encodes VP9 (`libvpx-vp9`) plus Opus (`libopus`) in yuv420p WebM. A bounded Tokio collector drains both child streams before results are interpreted. Process errors carry no child paths or raw stderr.

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

## Review-fix RED/GREEN

RED contracts were added for zero-duration rejection, Clap rejection of `--seconds 0`, controlled child collection, bounded timeout/reap behavior, and atomic evidence replacement. The initial compile failed as expected because `InvalidRequestedDuration`, `TimedOut`, `with_timeouts`, and `write_evidence_atomic` did not exist.

GREEN replaces all CPU fallback, build-ID, and ffprobe `Command::output` paths with one Tokio child collector. It uses piped stdout/stderr, concurrent bounded draining (discarding excess), `kill_on_drop`, and a timeout that kills then waits/reaps the child before returning a redacted typed error. Generation timeout is `seconds + 15s`, capped at ten minutes; utility commands use five seconds. Zero seconds is rejected before child spawn both by the API and by Clap.

The evidence writer now creates a temporary file in the destination directory, writes, flushes, syncs, and atomically persists it. Its contract confirms replacement leaves no temporary artifacts. The controlled Unix child tests validate exact generated arguments, build-ID and ffprobe execution, 128 KiB stderr flood draining, nonzero redaction, timeout termination/reap, and no unpinned FFmpeg execution.

## Second review-fix GREEN

Generation now has a specialized streaming progress drain: it feeds fixed chunks into `ProgressParser` and retains only speed sum/count plus the finished marker. A controlled child emits over 64 KiB of valid complete progress blocks and succeeds with the correct average, so normal long encodes are not limited by the utility stdout cap.

Both collectors use `try_join!` under a timeout; missing pipes, read/wait errors, parser errors, and timeouts invoke the same kill-plus-wait/reap cleanup before their redacted typed error is returned. Once a child has successfully waited, post-wait validation requires no kill.

After persist, Unix syncs the parent directory. Windows syncs the persisted destination; `tempfile` uses its platform MoveFileEx replacement behavior there. These steps improve durability while retaining the existing no-truncated-destination and temporary-file cleanup contract; they do not claim a stronger cross-platform fsync guarantee.
