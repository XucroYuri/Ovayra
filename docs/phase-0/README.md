# Phase 0 evidence

Phase 0 is experimental feasibility work, not a production interface commitment.

`packaging/phase-0-matrix.toml` defines the real-device evidence that must pass before the Phase 0 gate can succeed. Each JSON record belongs in `evidence/`, uses schema version 1, and must be redacted: never record credentials, upload URLs, prompts, model results, media paths, or file names.

Required records must finish with `pass`. `conditional` and `skipped` are not valid outcomes for required real-device evidence. The matrix deliberately includes only the supported macOS Apple Silicon, Windows x86-64, and glibc Linux desktop targets described by the approved design.
