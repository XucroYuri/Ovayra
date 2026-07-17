# Task 12 report: portable CI and protected devices

Implemented the hosted Phase 0 matrix for macOS 14, Windows 2025, and Ubuntu 24.04 with Rust formatting, clippy, tests, Linux Slint/tray dependencies, trusted Task 10 bundle binding, conditional CPU fallback, evidence linting, and 14-day sanitized artifact retention.

Implemented the protected `main`/manual self-hosted workflow for the six `TargetId` values. It binds every Task 10 producer to the current repository and commit before download, validates the bundle, runs CPU/hardware/forced-fallback media checks, the 120-second preview, typed preview threshold verification, tray normal/fallback, keyring, process-group cancellation, and evidence linting. Linux jobs assert actual non-SSH X11/Wayland desktop session variables.

Gemini is an independent `gemini-smoke` environment job for one macOS, Windows, and Linux desktop representative. It consumes the prior fallback artifact, masks the protected environment credential, uses separate stage/resume processes, and removes the encrypted checkpoint before lint/upload. No signing credential is requested by this workflow.

Local verification was run for the new lint/verifier integration tests, formatting, and the application clippy target. Full workspace verification remains the final repository-level check; protected self-hosted and Gemini runs cannot be executed locally.
