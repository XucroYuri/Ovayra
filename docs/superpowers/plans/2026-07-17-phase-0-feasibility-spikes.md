# Ovayra Phase 0 Feasibility Spikes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce runnable, measured proof that Ovayra's Slint preview, FFmpeg hardware/CPU media path, Gemini resumable upload, desktop platform integration, and signed LGPL-only distribution chain are viable on every supported platform before product feature development begins.

**Architecture:** Use one disposable `ovayra-spike` composition root over five small experimental crates. Each spike exposes a narrow interface, deterministic tests, a live command, and a redacted evidence record. GitHub-hosted runners prove portable code; protected self-hosted desktop runners prove hardware, UI, keyring, signing, and installer behavior. A final gate accepts only a complete required matrix and turns the evidence into five ADRs.

**Tech Stack:** Rust 1.93, Slint 1.17.1, Tokio 1.52.4, reqwest 0.13.4 with rustls, wiremock 0.6.5, keyring 4.1.5, command-group 5.0.1, AES-256-GCM, FFmpeg 8.1.2/ffprobe CLI, Gemini Files and `generateContent` REST APIs, cargo-packager 0.11.8, Minisign, GitHub Actions.

## Global Constraints

- Read `docs/superpowers/specs/2026-07-17-ovayra-local-client-design.md` and `docs/superpowers/plans/2026-07-17-ovayra-implementation-roadmap.md` before changing code.
- Phase 0 code is experimental. Promote interfaces based on accepted ADRs; do not rename entire spike crates into production crates.
- All dependency versions in the root workspace are exact pins. Update them only in a dedicated dependency commit after rerunning the complete Phase 0 gate.
- The Slint event loop and all component mutation stay on the main thread. Background code sends `Send` data and uses `slint::invoke_from_event_loop` for UI mutation.
- Use Slint's native `SystemTrayIcon`; do not introduce a second desktop event loop.
- FFmpeg/ffprobe are child processes. Never link FFmpeg libraries into the Rust process during Phase 0.
- A child cancellation test must prove the entire process group/job object dies, including grandchildren.
- Live Gemini commands read `OVAYRA_GEMINI_API_KEY` only from the environment or OS keyring. There is no API-key CLI argument.
- Upload-session URLs are sensitive. Their `Debug` output is redacted, and their persisted representation is AES-256-GCM ciphertext whose master key exists only in the OS credential store.
- Evidence rejects fields whose names contain `api_key`, `token`, `secret`, `password`, `upload_url`, `prompt`, `result`, `media_path`, or `file_name`.
- Live test media is generated locally from FFmpeg `lavfi`; no user media is used or uploaded.
- Gemini live smoke uses stable `gemini-3.1-flash-lite`, a low output cap, a dedicated low-quota key, and deletes the remote file after the test.
- Official-bundle validation fails if `ffmpeg -buildconf` contains `--enable-gpl` or `--enable-nonfree`, or if source/config/license/checksum/SBOM material is missing.
- CI artifacts contain reports, installer metadata, synthetic samples, and logs only after redaction. Secrets, private keys, upload-session URLs, and notarization credentials are never uploaded.
- A required real-device result may not be marked `Skipped` or `Conditional`. Missing hardware or credentials fail the Phase 0 gate instead of silently narrowing the product claim.

## Phase 0 Acceptance Thresholds

| Spike | Required pass condition |
| --- | --- |
| Slint preview | 640x360 RGBA at 24 fps for 120 seconds; p95 frame enqueue-to-apply latency <= 100 ms; single-slot backpressure; post-warmup RSS growth <= 64 MiB; window hides to tray and restores on every supported desktop session |
| Media | LGPL-only FFmpeg creates WebM VP9/Opus on all target OSes; ffprobe reports VP9 + Opus; every claimed hardware backend passes probe and a 10-second decode/filter/encode self-test on its required real device; forced hardware failure completes by CPU fallback |
| Gemini | Contract tests cover start, chunk, query-offset, resume, finalize, poll, generate, delete, timeout, 429 and 5xx; a two-process live upload resumes from the server offset, reaches `ACTIVE`, returns non-empty analysis, and deletes the file |
| Platform | Keyring binary-secret round trip and cleanup pass; encrypted checkpoint contains no plaintext URL; parent and grandchild die within 5 seconds; supported tray sessions pass; no-tray Linux keeps the window accessible and shows an explicit warning |
| Distribution | `.app`/`.dmg`, `.msi`, AppImage and `.deb` build on native runners; platform signature checks pass; update signature rejects one-byte tampering; FFmpeg source/buildconf/license/checksum/SBOM correspondence passes |

## Required Real-Device Matrix

| Evidence ID | Runner/session | Required capabilities |
| --- | --- | --- |
| `macos-arm64-vt` | macOS 14+ Apple Silicon desktop | VideoToolbox decode/encode, Keychain, Slint tray, signed/notarized app and DMG |
| `windows-x64-mf` | Windows 11 x86-64 desktop with Intel or AMD GPU | D3D11VA decode, Media Foundation H.264 encode, Credential Manager, Slint tray, signed MSI |
| `windows-x64-nvidia` | Windows 11 x86-64 desktop with supported NVIDIA GPU | NVDEC/NVENC self-test and forced CPU fallback |
| `linux-x64-vaapi-wayland` | Supported glibc Linux Wayland desktop | VAAPI, Secret Service, StatusNotifier tray or explicit fallback, AppImage/deb |
| `linux-x64-vaapi-x11` | Supported glibc Linux X11 desktop | VAAPI, Secret Service, tray behavior |
| `linux-x64-nvidia` | Supported glibc Linux desktop with NVIDIA GPU | NVDEC/NVENC self-test and forced CPU fallback |

GitHub-hosted `macos-14`, `windows-2025`, and `ubuntu-24.04` runners run deterministic tests and CPU smoke only. They do not substitute for the required real-device evidence above.

## Phase 0 File Map

```text
Ovayra/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml
├── apps/ovayra-spike/
│   ├── Cargo.toml
│   ├── build.rs
│   ├── assets/tray.svg
│   ├── ui/spike.slint
│   ├── src/
│   │   ├── main.rs
│   │   ├── cli.rs
│   │   ├── child_tree.rs
│   │   └── preview_app.rs
│   └── tests/process_group.rs
├── crates/spike-contracts/
│   ├── Cargo.toml
│   ├── src/{lib.rs,evidence.rs,matrix.rs}
│   └── tests/evidence_contract.rs
├── crates/spike-media/
│   ├── Cargo.toml
│   ├── src/{lib.rs,capability.rs,cpu_fallback.rs,ffmpeg.rs,progress.rs,preview.rs}
│   └── tests/{cpu_fallback.rs,hardware_plans.rs,progress_contract.rs}
├── crates/spike-gemini/
│   ├── Cargo.toml
│   ├── src/{lib.rs,client.rs,dto.rs,session.rs}
│   └── tests/resumable_contract.rs
├── crates/spike-platform/
│   ├── Cargo.toml
│   ├── src/{lib.rs,envelope.rs,keyring_store.rs,process_group.rs}
│   └── tests/envelope.rs
├── crates/spike-release/
│   ├── Cargo.toml
│   ├── src/{lib.rs,ffmpeg_policy.rs,manifest.rs,package.rs}
│   └── tests/{ffmpeg_policy.rs,manifest_signature.rs}
├── packaging/
│   ├── Packager.toml
│   ├── phase-0-matrix.toml
│   ├── icons/spike.svg
│   ├── ffmpeg.lock
│   ├── NOTICE.txt
│   └── policies/ffmpeg-lgpl.toml
├── scripts/
│   ├── build-ffmpeg-linux.sh
│   ├── build-ffmpeg-macos.sh
│   └── build-ffmpeg-windows.ps1
├── .github/workflows/
│   ├── phase-0-ci.yml
│   ├── phase-0-device.yml
│   ├── phase-0-ffmpeg.yml
│   └── phase-0-release.yml
├── docs/adr/0001..0005
└── docs/phase-0/{README.md,evidence/,feasibility-report.md}
```

---

### Task 1: Bootstrap the Pinned Rust Spike Workspace

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `deny.toml`
- Create: `apps/ovayra-spike/Cargo.toml`
- Create: `apps/ovayra-spike/src/main.rs`
- Create: `crates/spike-contracts/Cargo.toml`
- Create: `crates/spike-contracts/src/lib.rs`
- Create: `crates/spike-media/Cargo.toml`
- Create: `crates/spike-media/src/lib.rs`
- Create: `crates/spike-gemini/Cargo.toml`
- Create: `crates/spike-gemini/src/lib.rs`
- Create: `crates/spike-platform/Cargo.toml`
- Create: `crates/spike-platform/src/lib.rs`
- Create: `crates/spike-release/Cargo.toml`
- Create: `crates/spike-release/src/lib.rs`

- [ ] **Step 1: Add the pinned workspace manifest**

Create `Cargo.toml` with this dependency policy:

```toml
[workspace]
members = [
  "apps/ovayra-spike",
  "crates/spike-contracts",
  "crates/spike-media",
  "crates/spike-gemini",
  "crates/spike-platform",
  "crates/spike-release",
]
resolver = "3"

[workspace.package]
version = "0.0.1"
edition = "2024"
rust-version = "1.93"
license = "MIT OR Apache-2.0"
repository = "https://github.com/ovayra/ovayra"

[workspace.dependencies]
aes-gcm = "=0.11.0"
anyhow = "=1.0.103"
base64 = "=0.22.1"
clap = { version = "=4.6.2", features = ["derive", "env"] }
command-group = { version = "=5.0.1", features = ["with-tokio"] }
hex = "=0.4.3"
icns = { version = "=0.4.0", default-features = false, features = ["pngio"] }
ico = "=0.5.0"
keyring = "=4.1.5"
minisign-verify = "=0.2.5"
png = "=0.18.1"
predicates = "=3.1.4"
reqwest = { version = "=0.13.4", default-features = false, features = ["json", "rustls", "stream"] }
resvg = "=0.47.0"
semver = { version = "=1.0.28", features = ["serde"] }
serde = { version = "=1.0.228", features = ["derive"] }
serde_json = "=1.0.150"
sha2 = "=0.11.0"
slint = { version = "=1.17.1", default-features = false, features = ["std", "backend-winit", "renderer-femtovg", "renderer-software", "system-tray"] }
slint-build = "=1.17.1"
sysinfo = "=0.38.4"
tempfile = "=3.27.0"
thiserror = "=2.0.18"
tokio = { version = "=1.52.4", features = ["fs", "io-util", "macros", "process", "rt-multi-thread", "sync", "time"] }
toml = "=1.1.3+spec-1.1.0"
tracing = "=0.1.44"
tracing-subscriber = { version = "=0.3.23", features = ["env-filter", "fmt"] }
url = { version = "=2.5.8", features = ["serde"] }
wiremock = "=0.6.5"
zeroize = "=1.9.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = "deny"
pedantic = "warn"
```

- [ ] **Step 2: Pin the toolchain and make each crate inherit workspace policy**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.93.0"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

Each crate manifest must use `version.workspace = true`, `edition.workspace = true`, `rust-version.workspace = true`, `license.workspace = true`, and `[lints] workspace = true`. The UI crate alone gets a `build-dependencies.slint-build` entry.

Use this dependency allocation so adapters do not leak into the evidence schema:

| Crate | Workspace dependencies |
| --- | --- |
| `spike-contracts` | `serde`, `serde_json`, `thiserror`, `toml` |
| `spike-media` | `serde`, `serde_json`, `sha2`, `tempfile`, `thiserror`, `tokio`, `tracing` |
| `spike-gemini` | `reqwest`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `url`, dev `wiremock` |
| `spike-platform` | `aes-gcm`, `base64`, `command-group`, `keyring`, `serde`, `serde_json`, `sysinfo`, `thiserror`, `tokio`, `zeroize` |
| `spike-release` | `hex`, `minisign-verify`, `semver`, `serde`, `serde_json`, `sha2`, `thiserror`, `toml`, `url` |
| `ovayra-spike` | `anyhow`, `clap`, `icns`, `ico`, `png`, `resvg`, `slint`, `spike-*`, `tokio`, `tracing`, `tracing-subscriber`; build `slint-build` |

Create `deny.toml`:

```toml
[advisories]
yanked = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"
highlight = "all"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

License selection remains a separate explicit release-policy check because Slint's multi-license expression requires choosing the applicable community or royalty-free desktop terms rather than treating dependency metadata as a legal decision.

- [ ] **Step 3: Add minimal compilable crate roots and CLI**

Use this initial `apps/ovayra-spike/src/main.rs`:

```rust
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ovayra-spike", version, about = "Ovayra Phase 0 proof runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
}

fn main() {
    match Cli::parse().command {
        Command::Version => println!("ovayra-spike {}", env!("CARGO_PKG_VERSION")),
    }
}
```

Each library root initially contains `#![forbid(unsafe_code)]` and a one-line crate-level purpose comment.

- [ ] **Step 4: Generate and retain `Cargo.lock`**

Run:

```bash
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo install cargo-deny --version 0.20.2 --locked
cargo deny check advisories bans sources
cargo install cargo-audit --version 0.22.2 --locked
cargo audit
cargo install cargo-packager --version 0.11.8 --locked
cargo install cargo-cyclonedx --version 0.5.9 --locked
```

Expected: all four commands exit `0`; `Cargo.lock` is created and tracked.

- [ ] **Step 5: Commit the workspace skeleton**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml deny.toml apps crates
git commit -m "build: bootstrap phase 0 spike workspace"
```

---

### Task 2: Define Redacted Evidence and Matrix Contracts

**Files:**
- Create: `crates/spike-contracts/src/evidence.rs`
- Create: `crates/spike-contracts/src/matrix.rs`
- Modify: `crates/spike-contracts/src/lib.rs`
- Create: `crates/spike-contracts/tests/evidence_contract.rs`
- Create: `packaging/phase-0-matrix.toml`
- Create: `docs/phase-0/README.md`
- Create: `docs/phase-0/evidence/.gitkeep`

- [ ] **Step 1: Write the failing evidence contract tests**

Create `crates/spike-contracts/tests/evidence_contract.rs`:

```rust
use spike_contracts::{Evidence, EvidenceError, SpikeId, TargetId, Verdict};

#[test]
fn rejects_sensitive_measurement_names() {
    let mut evidence = Evidence::new(SpikeId::Gemini, TargetId::new("macos-arm64-vt"));
    let error = evidence.measure("upload_url", "https://secret.invalid").unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveField(_)));
}

#[test]
fn serializes_only_finished_evidence() {
    let evidence = Evidence::new(SpikeId::Preview, TargetId::new("macos-arm64-vt"));
    assert!(matches!(evidence.to_pretty_json(), Err(EvidenceError::Unfinished)));
}

#[test]
fn finished_report_has_stable_schema() {
    let mut evidence = Evidence::new(SpikeId::Media, TargetId::new("linux-x64-vaapi-wayland"));
    evidence.measure("p95_latency_ms", 18).unwrap();
    evidence.finish(Verdict::Pass, 1_250);
    let json = evidence.to_pretty_json().unwrap();
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"verdict\": \"pass\""));
}
```

- [ ] **Step 2: Run the tests and verify RED**

```bash
cargo test -p spike-contracts --test evidence_contract
```

Expected: compilation fails because `Evidence`, `SpikeId`, `TargetId`, and `Verdict` do not exist.

- [ ] **Step 3: Implement the evidence schema and sensitive-field guard**

Implement these public types in `evidence.rs`:

```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const FORBIDDEN: &[&str] = &[
    "api_key", "token", "secret", "password", "upload_url",
    "prompt", "result", "media_path", "file_name",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpikeId { Preview, Media, Gemini, Platform, Distribution }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict { Pass, Conditional, Fail, Skipped }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetId(String);

impl TargetId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub schema_version: u32,
    pub spike: SpikeId,
    pub target: TargetId,
    pub verdict: Option<Verdict>,
    pub duration_ms: Option<u64>,
    pub measurements: BTreeMap<String, Value>,
    pub observations: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("sensitive evidence field is forbidden: {0}")]
    SensitiveField(String),
    #[error("evidence has not been finished")]
    Unfinished,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Evidence {
    #[must_use]
    pub fn new(spike: SpikeId, target: TargetId) -> Self {
        Self {
            schema_version: 1, spike, target, verdict: None, duration_ms: None,
            measurements: BTreeMap::new(), observations: Vec::new(),
        }
    }

    pub fn measure(&mut self, name: &str, value: impl Serialize) -> Result<(), EvidenceError> {
        let normalized = name.to_ascii_lowercase();
        if FORBIDDEN.iter().any(|part| normalized.contains(part)) {
            return Err(EvidenceError::SensitiveField(name.to_owned()));
        }
        self.measurements.insert(name.to_owned(), serde_json::to_value(value)?);
        Ok(())
    }

    pub fn finish(&mut self, verdict: Verdict, duration_ms: u64) {
        self.verdict = Some(verdict);
        self.duration_ms = Some(duration_ms);
    }

    pub fn to_pretty_json(&self) -> Result<String, EvidenceError> {
        if self.verdict.is_none() || self.duration_ms.is_none() {
            return Err(EvidenceError::Unfinished);
        }
        Ok(serde_json::to_string_pretty(self)?)
    }
}
```

Re-export the types from `lib.rs`. Add `RequiredEvidence { id, target, session, backend }` and a TOML `PhaseZeroMatrix` loader in `matrix.rs`.

- [ ] **Step 4: Encode the complete required matrix**

`packaging/phase-0-matrix.toml` contains one `[[required]]` entry for each combination below:

```toml
[[required]]
id = "preview"
target = "macos-arm64-vt"
session = "aqua"

[[required]]
id = "media"
target = "macos-arm64-vt"
backend = "videotoolbox"

[[required]]
id = "media"
target = "windows-x64-mf"
backend = "d3d11va-mf"

[[required]]
id = "media"
target = "windows-x64-nvidia"
backend = "nvenc-nvdec"

[[required]]
id = "media"
target = "linux-x64-vaapi-wayland"
backend = "vaapi"

[[required]]
id = "media"
target = "linux-x64-nvidia"
backend = "nvenc-nvdec"

[[required]]
id = "platform"
target = "linux-x64-vaapi-wayland"
session = "wayland"

[[required]]
id = "platform"
target = "linux-x64-vaapi-x11"
session = "x11"
```

Add preview, platform, Gemini, CPU fallback, and distribution entries for every applicable target from the matrix table near the top of this plan. Do not encode unsupported targets.

- [ ] **Step 5: Verify GREEN and commit**

```bash
cargo test -p spike-contracts
git add crates/spike-contracts packaging/phase-0-matrix.toml docs/phase-0
git commit -m "test: define phase 0 evidence contracts"
```

Expected: all three evidence tests and matrix parsing tests pass.

---

### Task 3: Implement FFmpeg Process, Capability, and Progress Contracts

**Files:**
- Create: `crates/spike-media/src/ffmpeg.rs`
- Create: `crates/spike-media/src/progress.rs`
- Create: `crates/spike-media/src/capability.rs`
- Modify: `crates/spike-media/src/lib.rs`
- Create: `crates/spike-media/tests/progress_contract.rs`
- Create: `crates/spike-media/tests/hardware_plans.rs`

- [ ] **Step 1: Write failing progress parser tests**

```rust
use spike_media::{ProgressEvent, ProgressParser};

#[test]
fn parses_complete_progress_blocks_and_ignores_unknown_keys() {
    let input = b"frame=48\nout_time_us=2000000\nspeed=1.25x\nfuture_key=x\nprogress=continue\n";
    let events = ProgressParser::default().push(input).unwrap();
    assert_eq!(events, vec![ProgressEvent {
        frame: Some(48), out_time_us: Some(2_000_000),
        speed: Some(1.25), finished: false,
    }]);
}

#[test]
fn buffers_split_utf8_and_line_boundaries() {
    let mut parser = ProgressParser::default();
    assert!(parser.push(b"frame=1\nprogr").unwrap().is_empty());
    let events = parser.push(b"ess=end\n").unwrap();
    assert!(events[0].finished);
}
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p spike-media --test progress_contract
```

Expected: missing parser types cause compilation failure.

- [ ] **Step 3: Implement a block parser for `-progress pipe:1`**

`ProgressParser` owns a `Vec<u8>` pending buffer and a `BTreeMap<String, String>` current block. It consumes newline-delimited `key=value` pairs, emits only when it sees `progress=continue` or `progress=end`, accepts unknown keys, rejects malformed numeric values with a typed `ProgressError`, and caps pending input at 64 KiB.

The FFmpeg runner always supplies:

```rust
const COMMON_ARGS: &[&str] = &[
    "-hide_banner", "-nostdin", "-nostats", "-progress", "pipe:1",
];
```

Stderr is captured separately and capped at 1 MiB. Evidence stores the exit code and a SHA-256 of redacted stderr, never an unrestricted command log containing paths.

- [ ] **Step 4: Write failing hardware plan tests**

Assert these exact plan invariants:

```rust
use spike_media::{Backend, HardwarePlan};

#[test]
fn videotoolbox_plan_uses_platform_decoder_and_encoder() {
    let plan = HardwarePlan::self_test(Backend::VideoToolbox);
    assert!(plan.args().windows(2).any(|w| w == ["-hwaccel", "videotoolbox"]));
    assert!(plan.args().windows(2).any(|w| w == ["-c:v", "h264_videotoolbox"]));
}

#[test]
fn vaapi_plan_keeps_frames_on_the_hardware_surface() {
    let plan = HardwarePlan::self_test(Backend::Vaapi);
    assert!(plan.args().windows(2).any(|w| w == ["-hwaccel_output_format", "vaapi"]));
    assert!(plan.args().windows(2).any(|w| w == ["-c:v", "h264_vaapi"]));
}

#[test]
fn no_plan_claims_gpu_without_a_runtime_self_test() {
    for backend in Backend::ALL {
        assert!(HardwarePlan::self_test(backend).requires_observed_output());
    }
}
```

- [ ] **Step 5: Implement backend inventory and self-test plans**

Define the stable spike enum and command facts:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend { VideoToolbox, D3d11vaMf, NvencNvdec, Vaapi }

impl Backend {
    pub const ALL: [Self; 4] = [
        Self::VideoToolbox, Self::D3d11vaMf, Self::NvencNvdec, Self::Vaapi,
    ];
}
```

Inventory executes `ffmpeg -version`, `-buildconf`, `-hwaccels`, `-decoders`, `-encoders`, and `-filters`. Self-tests consume the same generated 10-second H.264/AAC input and use:

- VideoToolbox: `-hwaccel videotoolbox ... -vf scale=1280:720 -c:v h264_videotoolbox`.
- D3D11VA/MF: `-hwaccel d3d11va ... -vf scale=1280:720 -c:v h264_mf`.
- NVIDIA: `-hwaccel cuda ... -vf scale_cuda=1280:720 -c:v h264_nvenc`.
- VAAPI: `-hwaccel vaapi -hwaccel_output_format vaapi ... -vf scale_vaapi=w=1280:h=720 -c:v h264_vaapi` with an explicit render device argument.

A backend is `available` only when inventory names the required components and the self-test exits `0` with at least one output video frame. Availability is never inferred from OS or vendor alone.

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p spike-media --test progress_contract
cargo test -p spike-media --test hardware_plans
git add crates/spike-media
git commit -m "feat: add ffmpeg capability and progress contracts"
```

---

### Task 4: Prove the LGPL CPU WebM Fallback End to End

**Files:**
- Create: `crates/spike-media/src/cpu_fallback.rs`
- Create: `crates/spike-media/tests/cpu_fallback.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Modify: `apps/ovayra-spike/src/main.rs`

- [ ] **Step 1: Write a failing FFmpeg-backed integration test**

```rust
use spike_media::{CpuFallback, FfprobeReport};

#[test]
#[ignore = "requires the pinned Phase 0 ffmpeg bundle"]
fn produces_gemini_compatible_vp9_opus_webm() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("fallback.webm");
    CpuFallback::new("ffmpeg", "ffprobe").generate_synthetic(&output, 3).unwrap();
    let report = FfprobeReport::read("ffprobe", &output).unwrap();
    assert_eq!(report.container, "matroska,webm");
    assert_eq!(report.video_codec.as_deref(), Some("vp9"));
    assert_eq!(report.audio_codec.as_deref(), Some("opus"));
    assert_eq!(report.video_pixel_format.as_deref(), Some("yuv420p"));
}
```

- [ ] **Step 2: Run RED against a bundle without the implementation**

```bash
cargo test -p spike-media --test cpu_fallback -- --ignored --exact produces_gemini_compatible_vp9_opus_webm
```

Expected: compilation fails because `CpuFallback` and `FfprobeReport` are missing.

- [ ] **Step 3: Implement the exact fallback command**

The generated FFmpeg argument vector is:

```text
-y -hide_banner -nostdin
-f lavfi -i testsrc2=size=640x360:rate=24
-f lavfi -i sine=frequency=1000:sample_rate=48000
-t 10
-map 0:v:0 -map 1:a:0
-c:v libvpx-vp9 -deadline realtime -cpu-used 4 -b:v 600k -pix_fmt yuv420p
-c:a libopus -b:a 64k -ac 1
-f webm
-progress pipe:1 -nostats
target/phase-0/fallback.webm
```

`FfprobeReport::read` executes:

```text
ffprobe -v error -show_entries format=format_name,duration:stream=codec_name,codec_type,pix_fmt -of json target/phase-0/fallback.webm
```

It rejects a non-WebM container, missing audio/video stream, non-VP9 video, non-Opus audio, zero duration, or zero-byte output.

- [ ] **Step 4: Add the live CLI command and evidence**

Expose:

```text
ovayra-spike media cpu-fallback --ffmpeg "$OVAYRA_FFMPEG" --ffprobe "$OVAYRA_FFPROBE" --seconds 10 --output target/phase-0/fallback.webm --evidence docs/phase-0/evidence/cpu-fallback-local.json
```

The report includes duration, output bytes, average encode speed, codecs, pixel format, FFmpeg build ID, and verdict. It stores only the output content hash, not the local path.

- [ ] **Step 5: Run GREEN on each target OS and commit**

```bash
cargo test -p spike-media --test cpu_fallback -- --ignored
cargo run -p ovayra-spike -- media cpu-fallback \
  --ffmpeg "$OVAYRA_FFMPEG" --ffprobe "$OVAYRA_FFPROBE" \
  --seconds 10 --output target/phase-0/fallback.webm \
  --evidence docs/phase-0/evidence/cpu-fallback-local.json
git add crates/spike-media apps/ovayra-spike
git commit -m "feat: prove cpu webm fallback"
```

Expected final CLI line: `CPU_FALLBACK=PASS codec=vp9 audio=opus`.

---

### Task 5: Execute Hardware Self-Tests and Forced CPU Downgrade

**Files:**
- Modify: `crates/spike-media/src/capability.rs`
- Modify: `crates/spike-media/src/ffmpeg.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Create: `crates/spike-media/tests/fallback_policy.rs`

- [ ] **Step 1: Write the failing policy test**

```rust
use spike_media::{AttemptOutcome, Backend, ExecutionPolicy};

#[test]
fn hardware_failure_restarts_the_stage_on_cpu_and_records_reason() {
    let mut policy = ExecutionPolicy::prefer(Backend::NvencNvdec);
    let next = policy.observe(AttemptOutcome::Failed("device lost".into())).unwrap();
    assert!(next.is_cpu());
    assert_eq!(policy.downgrade_reason(), Some("device lost"));
    assert!(!policy.may_retry_hardware_in_this_session());
}
```

- [ ] **Step 2: Implement an explicit two-attempt policy**

The policy permits one preferred hardware attempt and one CPU attempt. A hardware probe failure, process spawn error, timeout, non-zero exit, missing frames, or invalid ffprobe result triggers CPU. The hardware backend is quarantined for the current process. CPU failure is terminal. Evidence records `requested_backend`, `actual_backend`, and a bounded `downgrade_code`; it never emits a generic `gpu=true` field.

- [ ] **Step 3: Add inventory and self-test commands**

```text
ovayra-spike media inventory --ffmpeg "$OVAYRA_FFMPEG" --evidence docs/phase-0/evidence/media-inventory-local.json
ovayra-spike media self-test --backend "$OVAYRA_BACKEND" \
  --ffmpeg "$OVAYRA_FFMPEG" --ffprobe "$OVAYRA_FFPROBE" \
  --input target/phase-0/hardware-input.mp4 --output target/phase-0/hardware-output.mp4 \
  --render-device "$OVAYRA_RENDER_DEVICE" --evidence docs/phase-0/evidence/media-self-test-local.json
ovayra-spike media forced-fallback --backend "$OVAYRA_BACKEND" --ffmpeg "$OVAYRA_FFMPEG" \
  --ffprobe "$OVAYRA_FFPROBE" --input target/phase-0/hardware-input.mp4 \
  --output target/phase-0/forced-fallback.webm --evidence docs/phase-0/evidence/media-fallback-local.json
```

`forced-fallback` injects an invalid hardware device, asserts the first attempt fails, then asserts the CPU WebM output passes ffprobe.

- [ ] **Step 4: Run deterministic tests and real-device commands**

```bash
cargo test -p spike-media
cargo run -p ovayra-spike -- media self-test --backend "$OVAYRA_BACKEND" \
  --ffmpeg "$OVAYRA_FFMPEG" --ffprobe "$OVAYRA_FFPROBE" \
  --input target/phase-0/hardware-input.mp4 --output target/phase-0/hardware-output.mp4 \
  --evidence "docs/phase-0/evidence/media-$OVAYRA_TARGET_ID-$OVAYRA_BACKEND.json"
cargo run -p ovayra-spike -- media forced-fallback --backend "$OVAYRA_BACKEND" \
  --ffmpeg "$OVAYRA_FFMPEG" --ffprobe "$OVAYRA_FFPROBE" \
  --input target/phase-0/hardware-input.mp4 --output target/phase-0/forced-fallback.webm \
  --evidence "docs/phase-0/evidence/fallback-$OVAYRA_TARGET_ID-$OVAYRA_BACKEND.json"
```

Expected: both commands exit `0`; self-test prints one of `ACTUAL_BACKEND=videotoolbox`, `d3d11va-mf`, `nvenc-nvdec`, or `vaapi`; forced fallback prints `ACTUAL_BACKEND=cpu` and `DOWNGRADE_OBSERVED=true`.

- [ ] **Step 5: Commit**

```bash
git add crates/spike-media apps/ovayra-spike
git commit -m "feat: validate hardware media fallback policy"
```

---

### Task 6: Prove Slint Preview, Main-Thread Updates, and Backpressure

**Files:**
- Create: `apps/ovayra-spike/build.rs`
- Create: `apps/ovayra-spike/ui/spike.slint`
- Create: `apps/ovayra-spike/assets/tray.svg`
- Create: `apps/ovayra-spike/src/preview_app.rs`
- Create: `crates/spike-media/src/preview.rs`
- Create: `crates/spike-media/tests/preview_frames.rs`
- Modify: `apps/ovayra-spike/src/main.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`

- [ ] **Step 1: Write failing frame validation and coalescing tests**

```rust
use spike_media::{Frame, LatestFrame};

#[test]
fn rgba_frame_requires_exact_byte_count() {
    assert!(Frame::rgba(2, 2, vec![0; 15], 1).is_err());
    assert!(Frame::rgba(2, 2, vec![0; 16], 1).is_ok());
}

#[test]
fn latest_frame_replaces_stale_work_instead_of_growing_a_queue() {
    let slot = LatestFrame::default();
    slot.publish(Frame::rgba(1, 1, vec![0, 0, 0, 255], 1).unwrap());
    slot.publish(Frame::rgba(1, 1, vec![1, 0, 0, 255], 2).unwrap());
    assert_eq!(slot.take().unwrap().sequence(), 2);
    assert!(slot.take().is_none());
}
```

- [ ] **Step 2: Implement a single-slot frame transport**

`Frame` contains width, height, RGBA bytes, sequence, and a monotonic enqueue timestamp. `LatestFrame` is `Arc<Mutex<Option<Frame>>>`; `publish` replaces the previous frame and increments an atomic dropped-frame counter. No unbounded channel is permitted.

`FfmpegPreview` executes:

```text
ffmpeg -hide_banner -nostdin -re -i target/phase-0/fallback.webm \
  -an -vf scale=640:360,fps=24 -pix_fmt rgba -f rawvideo pipe:1
```

It reads exactly `640 * 360 * 4` bytes per frame. A short frame at EOF ends cleanly; any other short read is an error. Stderr remains separately bounded.

- [ ] **Step 3: Build the Slint components**

Create `apps/ovayra-spike/build.rs`:

```rust
fn main() {
    slint_build::compile("ui/spike.slint").expect("compile Phase 0 Slint UI");
}
```

Create `apps/ovayra-spike/ui/spike.slint`:

```slint
export component PreviewWindow inherits Window {
    title: "Ovayra Preview Spike";
    width: 800px;
    height: 520px;
    in property <image> preview-frame;
    in property <string> status-text: "Waiting for frames";
    in property <string> metrics-text: "";

    VerticalLayout {
        padding: 16px;
        spacing: 8px;
        Text { text: root.status-text; font-size: 18px; }
        Image {
            source: root.preview-frame;
            image-fit: contain;
            horizontal-stretch: 1;
            vertical-stretch: 1;
        }
        Text { text: root.metrics-text; font-size: 13px; color: #666; }
    }
}

export component SpikeTray inherits SystemTrayIcon {
    icon: @image-url("../assets/tray.svg");
    tooltip: "Ovayra Phase 0";
    callback restore();
    callback quit();

    Menu {
        MenuItem { title: "Show Ovayra"; activated => { restore(); } }
        MenuSeparator { }
        MenuItem { title: "Quit"; activated => { quit(); } }
    }
}
```

The spike SVG is a simple opaque 64x64 rounded square with a contrasting `O`; it is explicitly a technical-test asset, not the final brand mark.

- [ ] **Step 4: Implement the main-thread frame bridge**

Use Slint's sendable pixel buffer and weak component handle:

```rust
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use spike_media::{Frame, LatestFrame};

pub struct FrameBridge {
    latest: LatestFrame,
    apply_queued: Arc<AtomicBool>,
    ui: Weak<PreviewWindow>,
}

impl FrameBridge {
    pub fn publish(&self, frame: Frame) {
        self.latest.publish(frame);
        schedule_apply(self.latest.clone(), self.apply_queued.clone(), self.ui.clone());
    }
}

fn schedule_apply(
    latest: LatestFrame,
    apply_queued: Arc<AtomicBool>,
    ui: Weak<PreviewWindow>,
) {
    if apply_queued.swap(true, Ordering::AcqRel) { return; }
    let retry_latest = latest.clone();
    let retry_flag = apply_queued.clone();
    let retry_ui = ui.clone();
    let queued_in_closure = apply_queued.clone();
    let result = slint::invoke_from_event_loop(move || {
        if let (Some(handle), Some(frame)) = (ui.upgrade(), latest.take()) {
            let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                frame.bytes(), frame.width(), frame.height(),
            );
            handle.set_preview_frame(Image::from_rgba8(pixels));
            handle.set_metrics_text(frame.metrics_text().into());
        }
        queued_in_closure.store(false, Ordering::Release);
        if !retry_latest.is_empty() {
            schedule_apply(retry_latest, retry_flag, retry_ui);
        }
    });
    if result.is_err() {
        apply_queued.store(false, Ordering::Release);
    }
}
```

Do not use `unsafe` or move an `Image` across threads. The compiled implementation must preserve the single queued UI closure invariant.

- [ ] **Step 5: Add automated measurement mode**

Expose:

```text
ovayra-spike preview --ffmpeg "$OVAYRA_FFMPEG" --input target/phase-0/fallback.webm \
  --duration-seconds 120 --automation --evidence docs/phase-0/evidence/preview-local.json
```

Automation starts the stream, hides and restores the window while the tray remains alive, then exits after 120 seconds. It records frames read/applied/dropped, p50/p95/p99 enqueue-to-apply latency, RSS at 20 and 120 seconds, renderer/backend, hide/restore outcome, and event-loop errors.

- [ ] **Step 6: Run tests and the live desktop gate**

```bash
cargo test -p spike-media --test preview_frames
cargo run -p ovayra-spike -- preview --ffmpeg "$OVAYRA_FFMPEG" \
  --input target/phase-0/fallback.webm --duration-seconds 120 --automation \
  --evidence "docs/phase-0/evidence/preview-$OVAYRA_TARGET_ID.json"
```

Expected final line starts with `PREVIEW=PASS fps=24` and reports numeric `p95_ms` no greater than `100` plus numeric `rss_growth_mib` no greater than `64`.

- [ ] **Step 7: Commit**

```bash
git add apps/ovayra-spike crates/spike-media
git commit -m "feat: prove slint preview frame bridge"
```

---

### Task 7: Implement Gemini Resumable Upload Contracts and Live Restart

**Files:**
- Create: `crates/spike-gemini/src/dto.rs`
- Create: `crates/spike-gemini/src/session.rs`
- Create: `crates/spike-gemini/src/client.rs`
- Modify: `crates/spike-gemini/src/lib.rs`
- Create: `crates/spike-gemini/tests/resumable_contract.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Modify: `apps/ovayra-spike/src/main.rs`

- [ ] **Step 1: Write failing request/response contract tests**

Use `wiremock` to require these behaviors:

```rust
#[tokio::test]
async fn starts_a_resumable_session_with_required_google_headers() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/upload/v1beta/files"))
        .and(wiremock::matchers::header("x-goog-upload-protocol", "resumable"))
        .and(wiremock::matchers::header("x-goog-upload-command", "start"))
        .and(wiremock::matchers::header("x-goog-upload-header-content-length", "8"))
        .and(wiremock::matchers::header("x-goog-upload-header-content-type", "video/webm"))
        .respond_with(wiremock::ResponseTemplate::new(200)
            .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri()))
            .insert_header("x-goog-upload-chunk-granularity", "4"))
        .mount(&server).await;

    let client = test_client(&server).await;
    let session = client.start_upload("synthetic", "video/webm", 8).await.unwrap();
    assert_eq!(session.chunk_granularity(), Some(4));
    assert_eq!(format!("{session:?}"), "UploadSession([REDACTED])");
}
```

Add separate tests for:

- chunk upload with `X-Goog-Upload-Offset` and `upload`;
- query with `X-Goog-Upload-Command: query`, reading `X-Goog-Upload-Size-Received`;
- final chunk with `upload, finalize`;
- poll transitions `PROCESSING -> ACTIVE` and terminal `FAILED`;
- `generateContent` with a `fileData` part and a text part after it;
- remote delete;
- bounded exponential retry for 429 and 5xx honoring `Retry-After`;
- no retry for 400/401/403;
- timeout while polling;
- debug/error text never containing the API key or upload URL.

- [ ] **Step 2: Run RED**

```bash
cargo test -p spike-gemini --test resumable_contract
```

Expected: compilation fails because the client and DTOs do not exist.

- [ ] **Step 3: Implement isolated Gemini DTOs and sensitive session type**

The public spike interface is:

```rust
pub struct GeminiClient { /* reqwest client, redacted credential, base URLs */ }
pub struct UploadSession { /* private Url, chunk granularity */ }
pub struct RemoteFile { pub name: String, pub uri: String, pub mime_type: String, pub state: FileState }
pub enum FileState { Unspecified, Processing, Active, Failed }

impl std::fmt::Debug for UploadSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UploadSession([REDACTED])")
    }
}
```

Use `https://generativelanguage.googleapis.com` as the API base and the same host's `/upload` prefix for upload start. Authentication uses the `x-goog-api-key` header. No DTO type escapes this crate; later production ports expose domain-owned types.

Upload chunks are aligned to the server-provided granularity except for the final chunk. If the response omits `x-goog-upload-chunk-granularity`, use an 8 MiB client chunk and verify the accepted offset with `query` before sending the next chunk. Before resuming, always query the server offset and seek the local file to that observed value. A mismatch between the persisted hint and server offset is recorded, with the server value authoritative.

Generation uses:

```text
POST /v1beta/models/gemini-3.1-flash-lite:generateContent
```

with a video `fileData` part followed by the concise prompt `Describe the synthetic test video in one sentence.` The smoke gate accepts a non-empty text part and records only response byte count, latency, model name, and HTTP status.

- [ ] **Step 4: Prove the protocol against the mock server**

```bash
cargo test -p spike-gemini --test resumable_contract
```

Expected: every start/chunk/query/finalize/poll/generate/delete/retry/redaction test passes without internet access.

- [ ] **Step 5: Add two-process live commands**

```text
ovayra-spike gemini stage-upload --input target/phase-0/fallback.webm \
  --checkpoint target/phase-0/upload-checkpoint.json --pause-after-chunks 1 \
  --evidence docs/phase-0/evidence/gemini-stage-local.json
ovayra-spike gemini resume-analyze --input target/phase-0/fallback.webm \
  --checkpoint target/phase-0/upload-checkpoint.json --model gemini-3.1-flash-lite \
  --evidence docs/phase-0/evidence/gemini-resume-local.json
```

`stage-upload` starts the session, uploads exactly one server-aligned chunk, encrypts the session checkpoint through `spike-platform`, and exits normally with a positive numeric `UPLOAD_PAUSED` value. `resume-analyze` is a separate process: it decrypts the checkpoint using the OS keyring, queries the server offset, proves it equals the staged value, resumes/finalizes, polls every 2 seconds up to 5 minutes, calls `generateContent`, deletes the file, and deletes the checkpoint.

- [ ] **Step 6: Run the controlled live gate**

```bash
cargo run -p ovayra-spike -- gemini stage-upload \
  --input target/phase-0/fallback.webm \
  --checkpoint target/phase-0/upload-checkpoint.json \
  --pause-after-chunks 1 \
  --evidence "docs/phase-0/evidence/gemini-stage-$OVAYRA_TARGET_ID.json"
cargo run -p ovayra-spike -- gemini resume-analyze \
  --input target/phase-0/fallback.webm \
  --checkpoint target/phase-0/upload-checkpoint.json \
  --model gemini-3.1-flash-lite \
  --evidence "docs/phase-0/evidence/gemini-resume-$OVAYRA_TARGET_ID.json"
```

Expected: `UPLOAD_RESUMED=true`, `REMOTE_STATE=ACTIVE`, `ANALYSIS_NONEMPTY=true`, and `REMOTE_DELETE=PASS`. Run this protected job on one macOS, one Windows, and one Linux target to expose TLS, proxy, filesystem, and keyring differences.

- [ ] **Step 7: Commit without live evidence containing service identifiers**

```bash
git add crates/spike-gemini apps/ovayra-spike
git commit -m "feat: prove gemini resumable upload contract"
```

---

### Task 8: Prove OS Keyring and Encrypted Upload Checkpoints

**Files:**
- Create: `crates/spike-platform/src/keyring_store.rs`
- Create: `crates/spike-platform/src/envelope.rs`
- Modify: `crates/spike-platform/src/lib.rs`
- Create: `crates/spike-platform/tests/envelope.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`

- [ ] **Step 1: Write failing in-memory envelope tests**

```rust
use spike_platform::{EncryptedRecord, EnvelopeCipher, MemorySecretStore, SecretStore};

#[test]
fn checkpoint_round_trip_never_serializes_plaintext() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let plaintext = b"https://upload.invalid/session/sensitive";
    let record = cipher.seal(plaintext, b"ovayra-upload-session-v1").unwrap();
    let json = serde_json::to_vec(&record).unwrap();
    assert!(!json.windows(plaintext.len()).any(|window| window == plaintext));
    assert_eq!(cipher.open(&record, b"ovayra-upload-session-v1").unwrap(), plaintext);
}

#[test]
fn tampering_is_rejected() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let mut record = cipher.seal(b"secret", b"context").unwrap();
    record.ciphertext[0] ^= 1;
    assert!(cipher.open(&record, b"context").is_err());
}
```

- [ ] **Step 2: Define the secret-store boundary**

```rust
pub trait SecretStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretStoreError>;
    fn set(&self, service: &str, account: &str, value: &[u8]) -> Result<(), SecretStoreError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretStoreError>;
}
```

`OsSecretStore` uses `keyring::v1::Entry::new(service, account)`, `set_secret`, `get_secret`, and `delete_credential`. Map a missing credential to `Ok(None)` and `NoDefaultStore`/locked/unavailable errors to explicit variants; never silently fall back to a plaintext file.

- [ ] **Step 3: Implement AES-256-GCM envelope records**

The master key is 32 random bytes generated through `aes_gcm::aead::Generate`, stored as service `com.ovayra.desktop` and account `installation-master-key-v1`. Each record uses a newly generated 96-bit nonce and associated data `ovayra-upload-session-v1`.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncryptedRecord {
    pub version: u8,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}
```

Reject versions other than `1`, nonce lengths other than 12 bytes, keys other than 32 bytes, authentication failure, and ciphertext larger than 16 KiB. Key material uses `zeroize` on drop; the platform crate consumes the workspace's exact `zeroize = "=1.9.0"` pin.

- [ ] **Step 4: Run deterministic tests**

```bash
cargo test -p spike-platform --test envelope
```

Expected: round trip and tamper rejection pass; a serialized record contains no plaintext.

- [ ] **Step 5: Add and run the native keyring smoke**

```text
ovayra-spike platform keyring --evidence docs/phase-0/evidence/keyring-local.json
```

The command writes 32 random bytes to a uniquely named Phase 0 account, reads and compares them, deletes the entry, then confirms a second get reports missing. It records only operation durations and error categories.

```bash
cargo run -p ovayra-spike -- platform keyring \
  --evidence "docs/phase-0/evidence/keyring-$OVAYRA_TARGET_ID.json"
```

Expected: `KEYRING=PASS cleanup=PASS`. On supported Linux, run inside the real desktop D-Bus session with an unlocked Secret Service; `NoDefaultStore` fails the required device gate.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/spike-platform apps/ovayra-spike
git commit -m "feat: protect upload sessions with os keyring"
```

---

### Task 9: Prove Tray Lifecycle, Linux Fallback, and Process-Tree Cancellation

**Files:**
- Create: `crates/spike-platform/src/process_group.rs`
- Create: `apps/ovayra-spike/tests/process_group.rs`
- Create: `apps/ovayra-spike/src/child_tree.rs`
- Modify: `apps/ovayra-spike/src/preview_app.rs`
- Modify: `apps/ovayra-spike/src/main.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`

- [ ] **Step 1: Write the failing process-group cancellation test**

```rust
use std::time::Duration;
use spike_platform::{GroupedProcess, ProcessTreeProbe};

#[tokio::test]
async fn cancellation_terminates_parent_and_grandchild() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree"]).await.unwrap();
    let pids = process.wait_for_reported_tree(Duration::from_secs(5)).await.unwrap();
    process.kill_and_wait(Duration::from_secs(5)).await.unwrap();
    assert!(!ProcessTreeProbe::any_alive(&pids));
}
```

Keep this test under the application package so Cargo defines `CARGO_BIN_EXE_ovayra-spike`; the process implementation itself remains in `spike-platform`.

- [ ] **Step 2: Implement a cross-platform Rust child tree**

The hidden `child-tree` command spawns the same executable with hidden `child-leaf`, prints one JSON line containing both PIDs, flushes stdout, and waits. `child-leaf` sleeps for 60 seconds. No shell-specific `sleep`, `ping`, or signal command is used.

`GroupedProcess::spawn` uses:

```rust
use command_group::AsyncCommandGroup;
use tokio::process::Command;

let mut command = Command::new(program);
command.args(args)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
let child = command.group_spawn()?;
```

`kill_and_wait` calls the grouped child's `kill().await` under a five-second Tokio timeout. On Unix this targets a process group; on Windows `command-group` uses a job object.

- [ ] **Step 3: Run the cancellation test on all OS runners**

```bash
cargo test -p ovayra-spike cancellation_terminates_parent_and_grandchild -- --exact
```

Expected: parent and grandchild both report dead within five seconds.

- [ ] **Step 4: Wire close-to-tray and explicit quit**

Create the tray first. If successful, set the main window close callback to `slint::CloseRequestResponse::HideWindow`; keep the tray strong handle alive on the main stack. Wire tray restore to `window.show()` and tray quit to `slint::quit_event_loop()`.

If tray creation fails or `--force-no-tray` is present:

- put the exact failure category in `status-text`;
- return `KeepWindowShown` from `on_close_requested` so no running task becomes inaccessible;
- show an explicit `Tray unavailable; keep this window open or choose Quit` banner;
- run until explicit quit.

This is the Phase 0 candidate for the Linux degradation rule. The ADR may accept it only after Wayland and X11 evidence passes.

- [ ] **Step 5: Run normal and forced-fallback lifecycle tests**

```bash
cargo run -p ovayra-spike -- platform tray --automation \
  --evidence "docs/phase-0/evidence/tray-$OVAYRA_TARGET_ID.json"
cargo run -p ovayra-spike -- platform tray --automation --force-no-tray \
  --evidence "docs/phase-0/evidence/tray-fallback-$OVAYRA_TARGET_ID.json"
```

Expected: normal mode prints `TRAY=PASS hide=PASS restore=PASS quit=PASS`; fallback prints `TRAY_FALLBACK=PASS window_accessible=true warning_visible=true`.

- [ ] **Step 6: Commit**

```bash
git add crates/spike-platform apps/ovayra-spike
git commit -m "feat: validate desktop lifecycle and process cancellation"
```

---

### Task 10: Build and Verify the LGPL-Only FFmpeg Provenance Chain

**Files:**
- Create: `packaging/ffmpeg.lock`
- Create: `packaging/policies/ffmpeg-lgpl.toml`
- Create: `crates/spike-release/src/ffmpeg_policy.rs`
- Modify: `crates/spike-release/src/lib.rs`
- Create: `crates/spike-release/tests/ffmpeg_policy.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Create: `.github/workflows/phase-0-ffmpeg.yml`
- Create: `packaging/NOTICE.txt`
- Create: `scripts/build-ffmpeg-macos.sh`
- Create: `scripts/build-ffmpeg-linux.sh`
- Create: `scripts/build-ffmpeg-windows.ps1`

- [ ] **Step 1: Write failing policy tests before writing build automation**

```rust
use spike_release::{FfmpegBundle, FfmpegPolicyError};

#[test]
fn rejects_gpl_and_nonfree_configurations() {
    for forbidden in ["--enable-gpl", "--enable-nonfree"] {
        let error = FfmpegBundle::validate_buildconf(&format!("configuration: {forbidden}"))
            .unwrap_err();
        assert!(matches!(error, FfmpegPolicyError::ForbiddenConfigureFlag(_)));
    }
}

#[test]
fn requires_corresponding_source_and_release_material() {
    let bundle = tempfile::tempdir().unwrap();
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::MissingArtifact(_)));
}
```

Add a valid-layout fixture test requiring all of:

```text
bin/ffmpeg[.exe]
bin/ffprobe[.exe]
provenance/ffmpeg-8.1.2.tar.xz
provenance/ffmpeg-8.1.2.tar.xz.asc
provenance/libvpx-source.tar.zst
provenance/opus-source.tar.zst
provenance/buildconf.txt
provenance/changes.diff
provenance/SHA256SUMS
LICENSES/FFmpeg-LGPL-2.1-or-later.txt
LICENSES/libvpx-BSD-3-Clause.txt
LICENSES/Opus-BSD-3-Clause.txt
NOTICE.txt
sbom/ffmpeg.cdx.json
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p spike-release --test ffmpeg_policy
```

Expected: missing validation types cause compilation failure.

- [ ] **Step 3: Pin source provenance, not third-party binaries**

`packaging/ffmpeg.lock` records:

- FFmpeg `8.1.2`, source `https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz`, detached signature URL, the verified tarball SHA-256, and the FFmpeg release-key fingerprint used for verification;
- libvpx tag `v1.16.0`, peeled commit `1024874c5919305883187e2953de8fcb4c3d7fa6`;
- Opus tag `v1.6.1`, peeled commit `22244de5a79bd1d6d623c32e72bf1954b56235be`;
- the native target triple and builder image/OS identity for each output.

The workflow downloads sources from those canonical origins, verifies FFmpeg's detached signature, verifies Git tag/commit identity for libvpx and Opus, computes hashes, and then writes the exact resolved values into the lock file. Do not place an unresolved checksum or release-key token in the file.

- [ ] **Step 4: Implement native reproducible build jobs**

All native builds export a fixed `SOURCE_DATE_EPOCH` from the FFmpeg release commit, build in a stable path, and strip nondeterministic debug prefixes. The common configure baseline is:

```text
--prefix="$OVAYRA_FFMPEG_STAGE"
--disable-autodetect
--disable-debug
--disable-doc
--disable-ffplay
--disable-network
--enable-ffmpeg
--enable-ffprobe
--enable-libopus
--enable-libvpx
--enable-version3
--disable-gpl
--disable-nonfree
```

Platform additions are:

- macOS: VideoToolbox and AudioToolbox enabled by the native configure probe;
- Windows: D3D11VA, Media Foundation, and nv-codec-headers for NVENC/NVDEC;
- Linux: VAAPI/DRM and nv-codec-headers for NVENC/NVDEC.

The two POSIX scripts use `set -euo pipefail`, accept source root, dependency prefix, stage root, and parallelism as named arguments, and never install into a system prefix. The PowerShell script uses `$ErrorActionPreference = 'Stop'` and the same logical arguments. Each script writes its fully expanded configure invocation to `provenance/buildconf.txt`, runs the upstream test targets for libvpx and Opus, then runs FFmpeg's `fate` smoke subset before staging. The scripts fail if their stage root exists but was not created for the current target triple, preventing cross-target overwrite.

The job fails if any desired component is absent from `-hwaccels`, `-decoders`, `-encoders`, or `-filters`. It also runs the CPU VP9/Opus test from Task 4. Build dependencies and their source/license material are copied into the provenance folder.

- [ ] **Step 5: Implement bundle-policy validation**

`FfmpegBundle::validate_layout` verifies required files, `SHA256SUMS`, buildconf policy, executable `-version`, source version correspondence, and CycloneDX component versions. It rejects symlinks escaping the bundle root and files not covered by the checksum manifest.

Expose:

```text
ovayra-spike release verify-ffmpeg --bundle "$OVAYRA_FFMPEG_BUNDLE" --evidence docs/phase-0/evidence/ffmpeg-bundle-local.json
```

- [ ] **Step 6: Build and compare twice per target**

The protected workflow performs two clean native builds and compares hashes. A byte-identical result passes. If platform signing or archive metadata prevents identical final package hashes, the unsigned FFmpeg/ffprobe binaries and normalized provenance tree must still be identical; document the exact nondeterministic packaging layer in ADR 0005.

```bash
cargo run -p ovayra-spike -- release verify-ffmpeg \
  --bundle "$OVAYRA_FFMPEG_BUNDLE" \
  --evidence "docs/phase-0/evidence/ffmpeg-bundle-$OVAYRA_TARGET_ID.json"
```

Expected: `FFMPEG_BUNDLE=PASS license=LGPL-only source_correspondence=PASS`.

- [ ] **Step 7: Commit**

```bash
git add packaging/ffmpeg.lock packaging/policies crates/spike-release .github/workflows/phase-0-ffmpeg.yml apps/ovayra-spike
git commit -m "build: prove lgpl-only ffmpeg distribution chain"
```

---

### Task 11: Prove Package Formats, Platform Signing, and Update Tamper Rejection

**Files:**
- Create: `packaging/Packager.toml`
- Create: `packaging/icons/spike.svg`
- Create: `crates/spike-release/src/manifest.rs`
- Create: `crates/spike-release/src/package.rs`
- Create: `crates/spike-release/tests/manifest_signature.rs`
- Create: `crates/spike-release/tests/fixtures/valid-manifest.json`
- Create: `crates/spike-release/tests/fixtures/update-test.bin`
- Create: `crates/spike-release/tests/fixtures/update-test.bin.minisig`
- Create: `crates/spike-release/tests/fixtures/update-test.pub`
- Modify: `crates/spike-release/src/lib.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Create: `.github/workflows/phase-0-release.yml`

- [ ] **Step 1: Write failing update-manifest and signature tests**

```rust
use spike_release::{ReleaseManifest, ReleaseVerifier};

#[test]
fn manifest_requires_only_supported_update_targets() {
    let json = include_str!("fixtures/valid-manifest.json");
    let manifest = ReleaseManifest::parse(json).unwrap();
    assert_eq!(manifest.version().to_string(), "0.0.1");
    assert_eq!(manifest.platform_count(), 3);
}

#[test]
fn one_byte_package_tampering_is_rejected() {
    let fixture = signed_fixture();
    ReleaseVerifier::new(fixture.public_key()).verify(fixture.package(), fixture.signature()).unwrap();
    let mut changed = fixture.package().to_vec();
    changed[0] ^= 1;
    assert!(ReleaseVerifier::new(fixture.public_key()).verify(&changed, fixture.signature()).is_err());
}
```

The valid manifest uses only `darwin-aarch64` format `app`, `windows-x86_64` format `wix`, and `linux-x86_64` format `appimage`. `.deb` appears as a download artifact but never as an in-place update target.

Generate the update-test fixture once with an ephemeral Minisign key, commit only the public key, payload, and signature, then destroy the private fixture key. `signed_fixture()` reads these three committed files; no signing secret is present in source control.

- [ ] **Step 2: Implement the manifest and Minisign verifier**

The JSON schema mirrors cargo-packager updater semantics:

```rust
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ReleaseManifest {
    version: semver::Version,
    pub_date: String,
    notes: String,
    platforms: std::collections::BTreeMap<String, PlatformRelease>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlatformRelease {
    url: url::Url,
    signature: String,
    format: UpdateFormat,
    sha256: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateFormat { App, Wix, Appimage }
```

Validate HTTPS, host exactly `updates.ovayra.com`, 64 lowercase hexadecimal SHA-256, allowed target/format pairing, unique URLs, and strictly newer SemVer. `ReleaseVerifier` uses `minisign_verify::{PublicKey, Signature}` and fails closed on parse, key-ID, signature, hash, or length mismatch.

- [ ] **Step 3: Configure cargo-packager**

Create top-level `packaging/Packager.toml`:

```toml
name = "ovayra-phase-0"
productName = "Ovayra Phase 0"
version = "0.0.1"
identifier = "com.ovayra.phase0"
beforePackagingCommand = "cargo build --release -p ovayra-spike"
binariesDir = "target/release"
outDir = "target/phase-0/packages"
icons = ["target/phase-0/icons/*"]
resources = [
  { src = "target/phase-0/ffmpeg-stage/**", target = "ffmpeg" },
  { src = "packaging/NOTICE.txt", target = "NOTICE.txt" },
]

[[binaries]]
path = "ovayra-spike"
main = true

[linux]
generateDesktopEntry = true
```

Before packaging, the release CLI validates and stages the target-specific FFmpeg bundle under `target/phase-0/ffmpeg-stage`. Generate required PNG/ICO/ICNS variants under `target/phase-0/icons/` from `spike.svg` deterministically in the release CLI; do not rely on an interactive graphics application in CI.

- [ ] **Step 4: Generate each native package format**

```bash
# Every native job prepares validated FFmpeg resources and icon variants first.
cargo run -p ovayra-spike -- release prepare-package \
  --bundle "$OVAYRA_FFMPEG_BUNDLE" --target "$OVAYRA_TARGET_TRIPLE"

# macOS Apple Silicon
cargo packager --release --config packaging/Packager.toml --formats app,dmg

# Windows x86-64
cargo packager --release --config packaging/Packager.toml --formats wix

# Linux x86-64
cargo packager --release --config packaging/Packager.toml --formats appimage,deb
```

Expected artifacts are a macOS `.app` and `.dmg`, Windows `.msi`, Linux `.AppImage` and `.deb`. Each package must contain the app, FFmpeg/ffprobe, Notice, license texts, exact source archive or a same-server source URL manifest, and SBOM.

- [ ] **Step 5: Exercise real platform signature verification**

Protected CI environments provide signing material; pull-request jobs never receive it.

macOS verification:

```bash
codesign --verify --deep --strict --verbose=2 target/phase-0/packages/Ovayra\ Phase\ 0.app
spctl --assess --type execute --verbose=4 target/phase-0/packages/Ovayra\ Phase\ 0.app
xcrun stapler validate target/phase-0/packages/*.dmg
```

Windows verification:

```powershell
$signature = Get-AuthenticodeSignature target/phase-0/packages/*.msi
if ($signature.Status -ne 'Valid') { throw "MSI signature is not valid: $($signature.Status)" }
```

Linux signs AppImage and `.deb` with the protected update Minisign key and verifies with the public key embedded in the spike verifier.

- [ ] **Step 6: Generate and corrupt-test an update manifest**

Use cargo-packager's signer in a protected job:

```bash
cargo packager signer sign --private-key "$CARGO_PACKAGER_SIGN_PRIVATE_KEY" \
  "$OVAYRA_UPDATE_ARTIFACT"
cargo run -p ovayra-spike -- release manifest \
  --packages target/phase-0/packages --base-url https://updates.ovayra.com/phase-0/ \
  --output target/phase-0/latest.json
cargo run -p ovayra-spike -- release verify-manifest \
  --manifest target/phase-0/latest.json --packages target/phase-0/packages
cargo run -p ovayra-spike -- release verify-tamper-rejection \
  --manifest target/phase-0/latest.json --packages target/phase-0/packages
```

Expected: `MANIFEST=PASS` followed by `TAMPER_REJECTION=PASS`. The corruption command works in a temporary copy and never alters the signed source artifact.

- [ ] **Step 7: Commit configuration and verifier, not keys or signed test artifacts**

```bash
git add packaging/Packager.toml packaging/icons crates/spike-release apps/ovayra-spike .github/workflows/phase-0-release.yml
git commit -m "build: validate signed desktop distribution"
```

---

### Task 12: Add Portable CI and Protected Real-Device Workflows

**Files:**
- Create: `.github/workflows/phase-0-ci.yml`
- Create: `.github/workflows/phase-0-device.yml`
- Modify: `.gitignore`
- Modify: `docs/phase-0/README.md`

- [ ] **Step 1: Add the GitHub-hosted deterministic matrix**

`phase-0-ci.yml` triggers on pull requests and pushes. Its matrix is `macos-14`, `windows-2025`, and `ubuntu-24.04`. Each job:

1. uses `actions/checkout@v6`;
2. lets `rust-toolchain.toml` install Rust 1.93 with rustfmt/clippy;
3. installs Linux Slint build dependencies when applicable;
4. runs `cargo fmt --all -- --check`;
5. runs `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
6. runs `cargo test --workspace --all-targets`;
7. runs CPU fallback against the Phase 0 bundle when present;
8. uploads only sanitized JSON reports and synthetic media with `actions/upload-artifact@v5`.

The Linux dependency step installs the compiler/linker packages plus Fontconfig, XKB, Wayland, X11, D-Bus, OpenGL, and AppIndicator development packages required by the selected Slint backend and system tray.

- [ ] **Step 2: Add the protected self-hosted device matrix**

`phase-0-device.yml` is `workflow_dispatch` plus protected `main` branch execution. Its matrix maps the six evidence IDs to explicit self-hosted labels. It runs CPU fallback, applicable hardware backend, preview, tray normal/fallback, keyring, process cancellation, and—only in the protected `gemini-smoke` environment—the two-process Gemini gate.

Each device job sets only non-secret identifiers in ordinary environment variables:

```yaml
env:
  OVAYRA_TARGET_ID: ${{ matrix.target_id }}
  OVAYRA_BACKEND: ${{ matrix.backend }}
  RUST_BACKTRACE: "1"
```

Gemini and signing secrets come from protected environment secrets, are masked, and are never passed on a command line. Linux Wayland and X11 jobs run inside their actual user desktop sessions rather than a headless SSH-only shell.

- [ ] **Step 3: Add artifact redaction and retention rules**

Before upload, run:

```bash
cargo run -p ovayra-spike -- evidence lint --dir docs/phase-0/evidence
```

The linter rejects forbidden key names and scans values for API-key prefixes, `x-goog-upload`, absolute home paths, URLs with query strings, and private-key headers. Evidence retention is 14 days. Package/notarization logs receive the same scan.

- [ ] **Step 4: Run the local CI equivalent**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo run -p ovayra-spike -- evidence lint --dir docs/phase-0/evidence
```

Expected: all commands exit `0`.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows .gitignore docs/phase-0
git commit -m "ci: add phase 0 platform evidence matrix"
```

---

### Task 13: Gate the Evidence, Write ADRs, and Freeze Phase 0 Decisions

**Files:**
- Modify: `crates/spike-contracts/src/matrix.rs`
- Modify: `apps/ovayra-spike/src/cli.rs`
- Create: `docs/adr/0001-slint-preview-and-event-loop.md`
- Create: `docs/adr/0002-ffmpeg-backends-and-cpu-fallback.md`
- Create: `docs/adr/0003-gemini-resumable-upload.md`
- Create: `docs/adr/0004-desktop-platform-integrations.md`
- Create: `docs/adr/0005-packaging-update-and-ffmpeg-distribution.md`
- Create: `docs/phase-0/feasibility-report.md`

- [ ] **Step 1: Write failing gate tests**

Test that the gate fails for a missing required record, duplicate conflicting records, `Conditional`, `Skipped`, `Fail`, wrong target/backend, stale schema, preview threshold violation, missing actual backend, plaintext-sensitive evidence, missing package format, and absent source correspondence. Test that a complete synthetic passing matrix succeeds.

```rust
#[test]
fn missing_required_evidence_fails_closed() {
    let matrix = fixture_matrix();
    let reports = vec![];
    let error = matrix.evaluate(&reports).unwrap_err();
    assert!(error.to_string().contains("missing required evidence"));
}
```

- [ ] **Step 2: Implement the final gate and report generator**

Expose:

```text
ovayra-spike gate --evidence-dir docs/phase-0/evidence --matrix packaging/phase-0-matrix.toml --report docs/phase-0/feasibility-report.md
```

The gate validates schema and redaction first, matches every required record by spike/target/session/backend, enforces the numeric thresholds at the top of this plan, and renders a Markdown table with source JSON hashes. Any unmatched or duplicate record is an error. The output contains no waiver mechanism.

- [ ] **Step 3: Collect protected workflow artifacts locally**

Download sanitized evidence from the successful device and release runs into `docs/phase-0/evidence/`. Commit only compact JSON proof and hashes; keep large binaries and detailed logs as workflow artifacts. Confirm every JSON file passes the evidence linter.

- [ ] **Step 4: Run the Phase 0 gate**

```bash
cargo run -p ovayra-spike -- gate \
  --evidence-dir docs/phase-0/evidence \
  --matrix packaging/phase-0-matrix.toml \
  --report docs/phase-0/feasibility-report.md
```

Expected final line: `PHASE_0_GATE=PASS`. Stop immediately on any other verdict and update the design before further implementation.

- [ ] **Step 5: Write ADR 0001 from passing preview evidence**

Record this accepted decision only if the preview gate passes:

> Ovayra uses Slint 1.17's main-thread event loop, `SharedPixelBuffer<Rgba8Pixel>` preview frames, a single replaceable pending frame, and `invoke_from_event_loop` for UI mutation. Slint's `SystemTrayIcon` keeps background work reachable after the main window hides.

Include measured per-platform latency, RSS, renderer, drop rates, tray result, the raw evidence hashes, and the fallback condition that would trigger a future custom renderer ADR.

- [ ] **Step 6: Write ADR 0002 from passing media evidence**

Record the exact FFmpeg version/config, backend command plans, devices/drivers, observed actual backends, fallback outputs, and the decision:

> Ovayra controls a bundled LGPL-only FFmpeg/ffprobe CLI through typed command plans, proves each claimed backend with runtime output, quarantines failed hardware for the session, and restarts the current stage with VP9/Opus WebM CPU fallback.

- [ ] **Step 7: Write ADR 0003 from passing Gemini evidence**

Record protocol headers, chunk granularity behavior, process-restart offset evidence, `ACTIVE` polling, model used, delete result, retry table, cost count, and the decision:

> Ovayra implements the Gemini Files resumable protocol directly in Rust, treats the server offset as authoritative, encrypts session URLs at rest, and separates upload resumability from generation attempts and their billing uncertainty.

- [ ] **Step 8: Write ADR 0004 from passing platform evidence**

Record Keychain/Credential Manager/Secret Service behavior, tray sessions, forced no-tray behavior, process group/job object results, and the decision:

> Ovayra uses OS-native credential stores, Slint's native tray integration, an accessible-window fallback when Linux tray registration fails, and grouped child processes for cancellation.

- [ ] **Step 9: Write ADR 0005 from passing distribution evidence**

Record FFmpeg provenance/reproducibility, cargo-packager formats, signature/notarization results, Minisign corruption rejection, SBOM/Notice/source layout, and the decision:

> Ovayra packages native artifacts with cargo-packager, verifies platform signatures plus a signed update manifest, never performs in-place `.deb` updates, and releases only policy-validated LGPL-only FFmpeg bundles with corresponding source and SBOM.

- [ ] **Step 10: Cross-check ADRs against the approved design**

```bash
rg -n "Conditional|Skipped|T[B]D|T[O]DO|place[h]older|waiver" docs/adr docs/phase-0
```

Expected: no unresolved marker in an accepted ADR or final report. Words appearing in a quoted test explanation must be rewritten so this command remains empty.

- [ ] **Step 11: Run the complete final verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo run -p ovayra-spike -- evidence lint --dir docs/phase-0/evidence
cargo run -p ovayra-spike -- gate \
  --evidence-dir docs/phase-0/evidence \
  --matrix packaging/phase-0-matrix.toml \
  --report docs/phase-0/feasibility-report.md
git status --short
```

Expected: all commands pass; the gate prints `PHASE_0_GATE=PASS`; `git status --short` shows only the intended ADR/report/evidence changes before commit.

- [ ] **Step 12: Commit and create the signed acceptance tag**

```bash
git add docs/adr docs/phase-0 crates/spike-contracts apps/ovayra-spike
git commit -m "docs: accept phase 0 feasibility evidence"
git tag -s phase-0-accepted -m "Ovayra Phase 0 accepted"
git tag -v phase-0-accepted
```

Expected: tag verification reports a good signature. Do not create this tag if any required record is missing or any ADR changes the approved product scope without a separately approved design revision.

---

## Authoritative Implementation References

- Slint 1.17.1 Rust threading and event loop: <https://docs.slint.dev/latest/docs/rust/slint/>
- Slint `Image::from_rgba8` and sendable pixel buffers: <https://docs.slint.dev/latest/docs/rust/slint/struct.Image>
- Slint `SystemTrayIcon`: <https://docs.slint.dev/latest/docs/slint/reference/window/systemtrayicon/>
- Gemini Files API resumable upload: <https://ai.google.dev/gemini-api/docs/files>
- Gemini video MIME and processing behavior: <https://ai.google.dev/gemini-api/docs/video-understanding>
- Gemini model lifecycle: <https://ai.google.dev/gemini-api/docs/models>
- FFmpeg CLI, progress, and hardware acceleration: <https://ffmpeg.org/ffmpeg.html>
- FFmpeg LGPL/GPL distribution checklist: <https://ffmpeg.org/legal.html>
- cargo-packager 0.11.8 formats and configuration: <https://docs.rs/cargo-packager/0.11.8/cargo_packager/>
- keyring 4.1.5 cross-platform credential stores: <https://docs.rs/keyring/4.1.5/keyring/v1/>

---

## After Phase 0

Once the signed `phase-0-accepted` tag exists, use `superpowers:writing-plans` to write `docs/superpowers/plans/2026-07-17-phase-1-open-core.md`. That plan must cite the five accepted ADRs and replace experimental details with production domain/application ports. Until then, do not implement SQLite product schema, project UI, licensing, updater installation, telemetry, or Pro workflows.
