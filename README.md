# Ovayra

Ovayra is a local-first, cross-platform video AI desktop workspace built with
Rust and Slint. Media processing, task execution, persistence, and recovery are
designed to run on the user's computer. Gemini requests go directly from the
client to Google's API; Ovayra product services must not receive Gemini keys,
prompts, media, model results, or project databases.

## Project status

Ovayra is currently in **Phase 0 feasibility validation**. The experimental
workspace implements the preview, FFmpeg media, resumable Gemini, desktop
platform, packaging, and fail-closed evidence contracts needed to validate the
architecture.

Phase 0 is presently **NO-GO** for product development because the protected
six-device, credentialed Gemini, native FFmpeg, platform-signing/notarization,
and updater evidence has not yet been collected. No accepted ADR or release tag
should be inferred from the presence of the spike code.

See:

- [Approved local-client design](docs/superpowers/specs/2026-07-17-ovayra-local-client-design.md)
- [Implementation roadmap](docs/superpowers/plans/2026-07-17-ovayra-implementation-roadmap.md)
- [Phase 0 implementation plan](docs/superpowers/plans/2026-07-17-phase-0-feasibility-spikes.md)
- [Evidence and protected-run guide](docs/phase-0/README.md)
- [Current feasibility report](docs/phase-0/feasibility-report.md)

## Supported release targets

- macOS 14 or later on Apple Silicon
- Windows 10/11 on x86-64
- glibc-based Linux x86-64 desktops using Wayland or X11

Intel macOS, Windows ARM64, Linux ARM64, musl/Alpine, headless Linux, Flatpak,
and Snap are outside the first-release support boundary.

## Local validation

The repository pins Rust 1.93 through `rust-toolchain.toml`.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo run --locked -p ovayra-spike -- evidence lint --dir docs/phase-0/evidence
```

The final evidence gate is intentionally fail-closed and will return
`PHASE_0_GATE=NO_GO` until every required protected proof is present.

## Open-core boundary

This public repository contains the community core. Commercial licensing,
automatic update services, optional crash reporting, and advanced workflows
belong in a separate private repository and may depend only on stable public
interfaces.

## License

Ovayra-authored source is available under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

Dependencies and bundled media components retain their own licenses. In
particular, FFmpeg distribution remains subject to the repository's LGPL-only
source-correspondence and release policy.

Commercial landing page: [ovayra.com](https://ovayra.com)
