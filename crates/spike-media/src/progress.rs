use std::collections::BTreeMap;

use thiserror::Error;

const MAX_PENDING_BYTES: usize = 64 * 1024;

/// A completed record emitted by `FFmpeg`'s `-progress pipe:1` protocol.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressEvent {
    pub frame: Option<u64>,
    pub out_time_us: Option<u64>,
    pub speed: Option<f64>,
    pub finished: bool,
}

/// Errors while parsing the small, line-oriented `FFmpeg` progress protocol.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProgressError {
    #[error("progress input exceeds the {limit}-byte pending-input limit")]
    PendingInputTooLarge { limit: usize },
    #[error("progress block exceeds the {limit}-byte limit")]
    BlockTooLarge { limit: usize },
    #[error("invalid numeric value for {key}")]
    InvalidNumber { key: String, value: String },
    #[error("invalid progress marker: {value}")]
    InvalidProgressMarker { value: String },
}

/// Incrementally parses `FFmpeg` progress output. Unknown fields are intentionally ignored.
#[derive(Debug, Default)]
pub struct ProgressParser {
    pending: Vec<u8>,
    current: BTreeMap<String, String>,
    current_bytes: usize,
}

impl ProgressParser {
    /// Adds arbitrary stdout bytes and returns only complete progress blocks.
    ///
    /// # Errors
    ///
    /// Returns an error for over-limit input, invalid markers, or malformed known numbers.
    pub fn push(&mut self, input: &[u8]) -> Result<Vec<ProgressEvent>, ProgressError> {
        let mut events = Vec::new();
        let mut start = 0;
        for (index, byte) in input.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            if let Err(error) = self.finish_line(&input[start..index], &mut events) {
                self.reset();
                return Err(error);
            }
            start = index + 1;
        }
        if start < input.len() {
            if self.pending.len().saturating_add(input.len() - start) > MAX_PENDING_BYTES {
                self.reset();
                return Err(ProgressError::PendingInputTooLarge {
                    limit: MAX_PENDING_BYTES,
                });
            }
            self.pending.extend_from_slice(&input[start..]);
        }
        Ok(events)
    }

    fn finish_line(
        &mut self,
        suffix: &[u8],
        events: &mut Vec<ProgressEvent>,
    ) -> Result<(), ProgressError> {
        if self.pending.len().saturating_add(suffix.len()) > MAX_PENDING_BYTES {
            return Err(ProgressError::PendingInputTooLarge {
                limit: MAX_PENDING_BYTES,
            });
        }
        self.pending.extend_from_slice(suffix);
        let mut line = std::mem::take(&mut self.pending);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        self.consume_line(&line, events)
    }

    fn reset(&mut self) {
        self.pending.clear();
        self.current.clear();
        self.current_bytes = 0;
    }

    fn consume_line(
        &mut self,
        line: &[u8],
        events: &mut Vec<ProgressEvent>,
    ) -> Result<(), ProgressError> {
        self.current_bytes = self.current_bytes.saturating_add(line.len() + 1);
        if self.current_bytes > MAX_PENDING_BYTES {
            return Err(ProgressError::BlockTooLarge {
                limit: MAX_PENDING_BYTES,
            });
        }

        let Some(separator) = line.iter().position(|byte| *byte == b'=') else {
            return Ok(());
        };
        let (key, value_with_separator) = line.split_at(separator);
        let value = &value_with_separator[1..];
        match key {
            b"frame" | b"out_time_us" | b"speed" | b"progress" => {
                let key = String::from_utf8_lossy(key).into_owned();
                let value = String::from_utf8(value.to_vec()).map_err(|_| {
                    ProgressError::InvalidNumber {
                        key: key.clone(),
                        value: String::from_utf8_lossy(value).into_owned(),
                    }
                })?;
                if key == "progress" {
                    self.emit(&value, events)?;
                } else {
                    self.current.insert(key, value);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn emit(&mut self, marker: &str, events: &mut Vec<ProgressEvent>) -> Result<(), ProgressError> {
        let finished = match marker {
            "continue" => false,
            "end" => true,
            _ => {
                return Err(ProgressError::InvalidProgressMarker {
                    value: marker.to_owned(),
                });
            }
        };
        let event = ProgressEvent {
            frame: self.parse_number("frame")?,
            out_time_us: self.parse_number("out_time_us")?,
            speed: self.parse_speed()?,
            finished,
        };
        self.current.clear();
        self.current_bytes = 0;
        events.push(event);
        Ok(())
    }

    fn parse_number(&self, key: &str) -> Result<Option<u64>, ProgressError> {
        self.current.get(key).map_or(Ok(None), |value| {
            value
                .parse()
                .map(Some)
                .map_err(|_| ProgressError::InvalidNumber {
                    key: key.to_owned(),
                    value: value.clone(),
                })
        })
    }

    fn parse_speed(&self) -> Result<Option<f64>, ProgressError> {
        self.current.get("speed").map_or(Ok(None), |value| {
            value
                .strip_suffix('x')
                .unwrap_or(value)
                .parse()
                .map(Some)
                .map_err(|_| ProgressError::InvalidNumber {
                    key: "speed".to_owned(),
                    value: value.clone(),
                })
        })
    }
}
