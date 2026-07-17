#![forbid(unsafe_code)]

//! Shared evidence contracts for the Phase 0 spikes.

mod evidence;
mod matrix;

pub use evidence::{Evidence, EvidenceError, SpikeId, TargetId, Verdict};
pub use matrix::{MatrixError, PhaseZeroMatrix, RequiredEvidence};
