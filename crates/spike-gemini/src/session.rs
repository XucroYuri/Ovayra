use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone)]
pub struct UploadSession {
    pub(crate) url: Url,
    pub(crate) chunk_granularity: Option<u64>,
}

impl UploadSession {
    #[must_use]
    pub fn chunk_granularity(&self) -> Option<u64> {
        self.chunk_granularity
    }
}

impl fmt::Debug for UploadSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UploadSession([REDACTED])")
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CheckpointPayload {
    pub url: String,
    pub chunk_granularity: Option<u64>,
    pub staged_offset: u64,
}

pub struct ResumedUpload {
    pub(crate) session: UploadSession,
    pub(crate) staged_offset: u64,
}

impl ResumedUpload {
    #[must_use]
    pub fn session(&self) -> &UploadSession {
        &self.session
    }

    #[must_use]
    pub fn staged_offset(&self) -> u64 {
        self.staged_offset
    }
}

impl fmt::Debug for ResumedUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResumedUpload([REDACTED])")
    }
}
