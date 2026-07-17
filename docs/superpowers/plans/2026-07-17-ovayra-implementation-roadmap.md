# Ovayra Implementation Roadmap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Ovayra as a local-first, cross-platform Rust desktop application through evidence-gated phases, without allowing unresolved desktop, media, Gemini, security, or distribution risks to leak into product implementation.

**Architecture:** Build one public Rust workspace around a modular single-process core and a Slint composition root. Keep business logic behind ports, run blocking work on background executors, use SQLite for durable state, invoke FFmpeg/ffprobe as controlled child-process groups, and call Gemini directly from the client. Add commercial capabilities later from a separate private repository through public capability traits.

**Tech Stack:** Rust 1.93, Cargo workspace, Slint 1.17.1, Tokio, SQLite/rusqlite, reqwest with rustls, FFmpeg/ffprobe CLI, Gemini REST API, OS-native credential stores, cargo-packager, GitHub Actions.

## Global Constraints

- The approved design is the source of truth: `docs/superpowers/specs/2026-07-17-ovayra-local-client-design.md`.
- The public repository is `ovayra/ovayra`; future closed-source code belongs in `ovayra/ovayra-pro` and may depend only on signed public tags or released crates.
- Supported release targets are macOS 14+ Apple Silicon, Windows 10/11 x86-64, and Linux x86-64 glibc on supported Wayland/X11 desktops.
- Intel macOS, Windows ARM64, Linux ARM64, musl/Alpine, headless Linux, Flatpak, and Snap are outside the first release.
- No product service may receive a Gemini key, prompt, media bytes, media path/name, model result, or project database.
- Gemini credentials, license leases, and installation master keys never enter SQLite, ordinary configuration, logs, crash reports, test snapshots, or CI artifacts.
- The official FFmpeg bundle must remain LGPL-only: neither `--enable-gpl` nor `--enable-nonfree` is allowed.
- Every long-running task exposes progress, cancellation, timeout, bounded retry, and recovery semantics.
- Every release-relevant hardware label records the backend actually used, not merely the requested backend.
- A phase starts only after the previous phase's exit gate is committed. Failed gates cause an ADR-backed architecture change before further product work.

---

## Plan Set and Dependency Order

```text
Approved design
  -> Phase 0 feasibility spikes
      -> ADRs + evidence gate
          -> Phase 1 open core
              -> Phase 2 desktop workbench
                  -> Phase 3 commercial productization
                      -> Phase 4 public beta and launch readiness
```

| Phase | Detailed plan | Entry gate | Exit gate |
| --- | --- | --- | --- |
| 0 | `docs/superpowers/plans/2026-07-17-phase-0-feasibility-spikes.md` | Approved design committed | Five spike ADRs accepted on all required platforms |
| 1 | `docs/superpowers/plans/2026-07-17-phase-1-open-core.md` | Phase 0 ADRs accepted | Headless local workflow passes recovery and privacy tests |
| 2 | `docs/superpowers/plans/2026-07-17-phase-2-desktop-workbench.md` | Open-core application services stable | Community desktop workflow passes three-platform UX acceptance |
| 3 | `docs/superpowers/plans/2026-07-17-phase-3-commercial-productization.md` | Public capability API tagged | Pro composition, signed packages, update and entitlement gates pass |
| 4 | `docs/superpowers/plans/2026-07-17-phase-4-public-beta.md` | Signed release candidate exists | Public-version acceptance criteria and launch checklist pass |

Only the Phase 0 file is written in executable detail now. The remaining detailed plans are deliberately generated after Phase 0 decisions, so they use measured preview, hardware, platform, network, and packaging constraints instead of assumptions.

## Intended Repository Shape

```text
Ovayra/
├── apps/
│   ├── ovayra/                  # community Slint application composition root
│   └── ovayra-spike/            # Phase 0 executable; removed after Phase 2 stabilizes
├── crates/
│   ├── ovayra-domain/           # entities, value objects, state transitions
│   ├── ovayra-application/      # use cases, commands, events, ports
│   ├── ovayra-storage/          # SQLite repositories, migrations, backup/recovery
│   ├── ovayra-media/            # ffprobe, FFmpeg plans, progress, GPU policy
│   ├── ovayra-gemini/           # Files API and generation REST adapter
│   ├── ovayra-platform/         # keyring, paths, tray, notifications, process groups
│   ├── ovayra-ui/               # shared Slint components and presenters
│   ├── ovayra-release/          # public manifest/signature and release policy types
│   └── spike-contracts/         # Phase 0 evidence schema
├── migrations/                  # immutable SQLite migrations
├── packaging/                   # cargo-packager config, icons, notices, policies
├── scripts/                     # release verification and reproducible tooling
├── tests/
│   ├── contract/                # adapter/provider contracts
│   ├── integration/             # headless workflows
│   └── recovery/                # crash/restart fault injection
├── docs/
│   ├── adr/                     # accepted architecture decisions
│   ├── phase-0/                 # generated, non-secret spike evidence
│   ├── support/                 # supported/excluded device documentation
│   └── superpowers/             # approved specifications and implementation plans
└── .github/workflows/           # CI, device gates, packaging, release gates
```

The Phase 0 workspace uses temporary `spike-*` crates. Phase 1 promotes only contracts that survived the spikes; it does not rename experimental implementation wholesale.

---

### Roadmap Task 1: Complete Phase 0 Feasibility Spikes

**Files:**
- Execute: `docs/superpowers/plans/2026-07-17-phase-0-feasibility-spikes.md`
- Produce: `docs/adr/0001-slint-preview-and-event-loop.md`
- Produce: `docs/adr/0002-ffmpeg-backends-and-cpu-fallback.md`
- Produce: `docs/adr/0003-gemini-resumable-upload.md`
- Produce: `docs/adr/0004-desktop-platform-integrations.md`
- Produce: `docs/adr/0005-packaging-update-and-ffmpeg-distribution.md`
- Produce: `docs/phase-0/feasibility-report.md`

- [ ] Run every deterministic unit and contract test on all three GitHub-hosted OS runners.
- [ ] Run hardware, tray, keyring, signing, installer, and Gemini smoke gates on the real-device matrix defined in the detailed Phase 0 plan.
- [ ] Record measurements as machine-readable JSON and summarize them without secrets in `docs/phase-0/feasibility-report.md`.
- [ ] Accept each ADR only when its explicit release gate passes; otherwise revise the design specification and rerun the affected spike.
- [ ] Tag the accepted evidence commit as `phase-0-accepted` with a signed tag.

**Phase 0 exit command:**

```bash
cargo run -p ovayra-spike -- gate \
  --evidence-dir docs/phase-0/evidence \
  --matrix packaging/phase-0-matrix.toml
```

Expected: exit code `0` and a final line `PHASE_0_GATE=PASS`. `CONDITIONAL`, `SKIPPED`, or missing required evidence fails the gate.

---

### Roadmap Task 2: Write and Execute Phase 1 Open-Core Plan

**Files:**
- Create: `docs/superpowers/plans/2026-07-17-phase-1-open-core.md`
- Create: `crates/ovayra-domain/`
- Create: `crates/ovayra-application/`
- Create: `crates/ovayra-storage/`
- Create: `crates/ovayra-media/`
- Create: `crates/ovayra-gemini/`
- Create: `crates/ovayra-platform/`
- Create: `crates/ovayra-release/`
- Create: `migrations/`
- Create: `tests/integration/`
- Create: `tests/recovery/`

- [ ] Use Phase 0 ADRs to define stable public ports for preview frames, media execution, Gemini uploads, secure storage, process cancellation, and release manifests.
- [ ] Implement domain state transitions before persistence or UI adapters.
- [ ] Implement SQLite WAL storage, immutable migrations, backup, integrity checks, and crash recovery.
- [ ] Implement in-place media references and optional content-addressed managed copies.
- [ ] Implement FFmpeg/ffprobe adapters with exact command recording, progress parsing, stage-boundary retry, and CPU fallback.
- [ ] Implement Gemini Files API upload sessions, remote-state polling, generation attempts, uncertainty markers, and explicit retry decisions.
- [ ] Prove the complete import -> normalize -> upload -> analyze -> export flow through headless integration tests.
- [ ] Prove that killing the process at every durable stage resumes safely without silently repeating an uncertain Gemini charge.

**Phase 1 exit command:**

```bash
cargo test --workspace --all-targets
cargo test -p ovayra-recovery-tests -- --ignored --test-threads=1
```

Expected: all tests pass; the ignored recovery suite is explicitly run and reports no secret leakage or invalid state transition.

---

### Roadmap Task 3: Write and Execute Phase 2 Desktop Workbench Plan

**Files:**
- Create: `docs/superpowers/plans/2026-07-17-phase-2-desktop-workbench.md`
- Create: `apps/ovayra/`
- Create: `crates/ovayra-ui/`
- Create: `crates/ovayra-ui/ui/`
- Create: `tests/ui/`

- [ ] Implement project, asset, queue, prompt, result, question/answer, export, and settings surfaces with Slint.
- [ ] Keep presenters dependent on application-service ports; no Slint callback may access SQLite, FFmpeg, Gemini, or the filesystem directly.
- [ ] Reuse the accepted Phase 0 preview transport and main-thread frame update model.
- [ ] Implement close-to-tray, explicit quit, task-running status, and the accepted Linux no-tray fallback.
- [ ] Display actual media backend, downgrade reason, upload state, Gemini state, and cost uncertainty.
- [ ] Add keyboard navigation, high-DPI, reduced-motion, long-path, Unicode, and accessibility checks.
- [ ] Run scripted community-edition acceptance flows on all supported desktop targets.

**Phase 2 exit command:**

```bash
cargo test -p ovayra-ui --all-targets
cargo run -p ovayra -- --acceptance-script tests/ui/community-workflow.json
```

Expected: `COMMUNITY_WORKFLOW=PASS`, with screenshots and accessibility output stored as CI artifacts.

---

### Roadmap Task 4: Write and Execute Phase 3 Commercial Productization Plan

**Files:**
- Create in public repo: `docs/superpowers/plans/2026-07-17-phase-3-commercial-productization.md`
- Create in public repo: `crates/ovayra-application/src/capabilities/`
- Create in private repo: `apps/ovayra-pro/`
- Create in private repo: `crates/ovayra-entitlement/`
- Create in private repo: `crates/ovayra-updater/`
- Create in private repo: `crates/ovayra-telemetry/`
- Create in private repo: `crates/ovayra-pro-workflows/`

- [ ] Freeze minimal public capability traits for entitlement, updates, optional crash reporting, and Pro workflow registration.
- [ ] Prove the public community app compiles and runs with null implementations and no Ovayra service dependency.
- [ ] Depend on a signed public tag from the private repository; do not copy or patch public source inside the private repository.
- [ ] Implement offline-tolerant signed license leases without making activation a startup single point of failure.
- [ ] Implement signed update checking and rollback protection for macOS, Windows, and AppImage; make `.deb` check-and-download only.
- [ ] Implement opt-in crash reporting with local preview, redaction tests, and hard rejection of forbidden payload fields.
- [ ] Generate notarized/signed installers, Notices, FFmpeg source/config bundles, checksums, and SBOMs.

**Phase 3 exit command:**

```bash
./scripts/verify-release-bundle target/release-bundle
```

Expected: `RELEASE_BUNDLE=PASS`; unsigned artifacts, missing source correspondence, forbidden licenses, or forbidden telemetry fields fail closed.

---

### Roadmap Task 5: Write and Execute Phase 4 Public Beta Plan

**Files:**
- Create: `docs/superpowers/plans/2026-07-17-phase-4-public-beta.md`
- Create: `docs/support/supported-platforms.md`
- Create: `docs/support/excluded-devices.md`
- Create: `docs/support/privacy-and-networking.md`
- Create: `docs/support/recovery-and-cost-uncertainty.md`
- Update external site: `https://ovayra.com`

- [ ] Exercise install, upgrade, rollback, uninstall, tray, sleep/wake, GPU fallback, keyring, and crash recovery on the complete supported matrix.
- [ ] Test corrupt databases, moved assets, expired Gemini files, partial uploads, offline launches, update signature failures, and incompatible schemas.
- [ ] Audit application, logs, crash packages, service requests, and release artifacts for prohibited data.
- [ ] Verify Ovayra.com and in-app support/exclusion wording are generated from the same versioned support data.
- [ ] Run a limited beta with explicit diagnostic consent and a documented support/escalation loop.
- [ ] Require all ten acceptance conditions from the approved design before publishing the first public release.

**Phase 4 exit command:**

```bash
./scripts/release-gate --candidate target/release-bundle --matrix docs/support/matrix.json
```

Expected: `PUBLIC_RELEASE_GATE=PASS` and no waived critical condition.

---

## Planning and Review Rules for Later Phases

- Write each detailed phase plan with `superpowers:writing-plans` only after its entry gate passes.
- Start each detailed plan by reading the approved design, every accepted prior ADR, and the preceding feasibility report.
- Preserve exact file paths, test-first steps, commands, expected output, and frequent atomic commits.
- If a later discovery contradicts an accepted ADR, add a superseding ADR and update this roadmap before implementation continues.
- Do not infer private-repository authorization from work in the public repository; private work begins only when the user supplies its path and access.
