#![forbid(unsafe_code)]

//! Shared evidence contracts for the Phase 0 spikes.

mod evidence;
mod matrix;
mod proof;

pub use evidence::{Evidence, EvidenceError, SpikeId, TargetId, TargetIdError, Verdict};
pub use matrix::{MatrixError, PhaseZeroMatrix, RequiredEvidence};
pub use proof::*;
