//! Incremental raw SSE framing. Limits include comments, ignored fields and delimiters.
use crate::error::ApiError;

pub(super) struct ChatSseFramer {
    line: Vec<u8>,
    data: String,
    has_data: bool,
    frame_bytes: usize,
    raw_bytes: usize,
    frames: usize,
    raw_limit: usize,
    frame_limit: usize,
}

impl ChatSseFramer {
    pub(super) fn new(raw_limit: usize, frame_limit: usize) -> Self {
        Self {
            line: Vec::new(),
            data: String::new(),
            has_data: false,
            frame_bytes: 0,
            raw_bytes: 0,
            frames: 0,
            raw_limit,
            frame_limit,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, ApiError> {
        self.raw_bytes = self
            .raw_bytes
            .checked_add(bytes.len())
            .ok_or_else(invalid_framing)?;
        if self.raw_bytes > self.raw_limit {
            return Err(invalid_framing());
        }
        let mut frames = Vec::new();
        for &byte in bytes {
            self.frame_bytes += 1;
            if self.frame_bytes > self.frame_limit {
                return Err(invalid_framing());
            }
            if byte == b'\n' {
                let line_bytes = self.line.strip_suffix(b"\r").unwrap_or(&self.line);
                if line_bytes.contains(&b'\r') {
                    return Err(invalid_framing());
                }
                let line = std::str::from_utf8(line_bytes).map_err(|_| invalid_framing())?;
                if line.is_empty() {
                    self.frames += 1;
                    if self.frames > 4096 {
                        return Err(invalid_framing());
                    }
                    if self.has_data {
                        self.data.pop(); // SSE appends then removes exactly one final LF.
                        frames.push(std::mem::take(&mut self.data));
                        self.has_data = false;
                    }
                    self.frame_bytes = 0;
                } else if !line.starts_with(':') {
                    let (field, value) = line.split_once(':').unwrap_or((line, ""));
                    if field == "data" {
                        self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
                        self.data.push('\n');
                        self.has_data = true;
                    }
                }
                self.line.clear();
            } else {
                self.line.push(byte);
            }
        }
        Ok(frames)
    }

    pub(super) fn finish_eof(&self) -> Result<(), ApiError> {
        if self.line.is_empty() && !self.has_data && self.frame_bytes == 0 {
            Ok(())
        } else {
            Err(invalid_framing())
        }
    }
}

fn invalid_framing() -> ApiError {
    ApiError::Stream("invalid or oversized GLM SSE framing".into())
}

#[cfg(test)]
#[path = "claudex_chat_framing_tests.rs"]
mod tests;
