#![deny(unsafe_code)]

//! The isolated, build-generated Slint component surface for the preview spike.

// Slint 1.17's generated item-tree macro emits this lint allowance internally.
// The exception is limited to generated code; this package's handwritten code is denied.
#[allow(unsafe_code)]
mod generated {
    slint::include_modules!();
}

pub use generated::{PreviewWindow, SpikeTray};
