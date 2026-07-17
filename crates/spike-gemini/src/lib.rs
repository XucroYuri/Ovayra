#![forbid(unsafe_code)]

//! Gemini adapter spike support.

mod client;
mod dto;
mod session;

pub use client::{GeminiClient, GeminiError, GenerationResult, PollPolicy, RetryPolicy};
pub use session::{ResumedUpload, UploadSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Unspecified,
    Processing,
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
    pub state: FileState,
}
