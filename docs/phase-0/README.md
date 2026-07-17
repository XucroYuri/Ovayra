# Phase 0 evidence

Phase 0 is experimental feasibility work, not a production interface commitment.

`packaging/phase-0-matrix.toml` defines the real-device evidence that must pass before the Phase 0 gate can succeed. Each JSON record belongs in `evidence/`, uses schema version 1, and must be redacted: never record credentials, upload URLs, prompts, model results, media paths, or file names.

Required records must finish with `pass`. `conditional` and `skipped` are not valid outcomes for required real-device evidence. The matrix deliberately includes only the supported macOS Apple Silicon, Windows x86-64, and glibc Linux desktop targets described by the approved design.

## CI and evidence handling

`phase-0-ci.yml` runs deterministic formatting, linting, and tests on GitHub-hosted `macos-14`, `windows-2025`, and `ubuntu-24.04`. A hosted CPU fallback runs only after that job downloads and validates a successful same-commit Task 10 FFmpeg producer artifact. Pull-request jobs therefore report an explicit skipped CPU exercise; a skip never creates passing CPU evidence and does not replace device evidence.

`phase-0-device.yml` runs only from `main` or an explicitly supplied producer run. It is intentionally not a pull-request workflow: its self-hosted labels select the six real devices below, bind the Task 10 producer to this repository and commit, and keep Gemini in the protected `gemini-smoke` environment. The workflow definition is implemented, but this repository contains no claim that a protected self-hosted execution has run; JSON evidence is collected only after such runs pass.

| Target ID | Supported session and backend | Excluded from this target |
| --- | --- | --- |
| `macos-arm64-vt` | Apple Silicon Aqua desktop; VideoToolbox | Intel macOS and headless sessions |
| `windows-x64-mf` | Windows desktop; D3D11VA/Media Foundation | ARM Windows and headless sessions |
| `windows-x64-nvidia` | Windows desktop with supported NVIDIA GPU; NVDEC/NVENC | non-NVIDIA Windows hardware |
| `linux-x64-vaapi-wayland` | glibc Wayland user session; VAAPI | SSH/headless and non-Wayland sessions |
| `linux-x64-vaapi-x11` | glibc X11 user session; VAAPI | SSH/headless and non-X11 sessions |
| `linux-x64-nvidia` | glibc desktop with supported NVIDIA GPU; NVDEC/NVENC | non-NVIDIA Linux hardware |

The evidence linter follows only bounded regular files and rejects symlinks, malformed or duplicate-key JSON, unsafe nested key names, credentials, upload endpoints, home paths, and URLs with credentials. It emits only a relative file name and category. It accepts an empty evidence directory; a missing directory fails closed.

Run the local equivalent before review:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo run --locked -p ovayra-spike -- evidence lint --dir docs/phase-0/evidence
```

Package or notarization logs must use bounded text mode before upload:

```bash
cargo run --locked -p ovayra-spike -- evidence lint --dir target/phase-0/package-logs --text
```

The device workflow separately checks the typed preview JSON with `evidence verify-preview`; it requires 120 seconds, 23--25 FPS, hide/restore, p95 at most 100 ms, RSS growth at most 64 MiB, and complete RSS samples. It does not accept a console `PASS` line as proof.
