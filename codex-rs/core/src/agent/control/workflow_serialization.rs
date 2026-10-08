//! Count JSON bytes with the codec's bounded-write pattern; allocate no JSON buffer.
use std::io::Write;

use serde::Serialize;

use super::MAX_WORKFLOW_JSON_BYTES;

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) enum EncodingError {
    Limit,
    Invalid,
}

pub(in super::super) fn check_json_budget<T: Serialize + ?Sized>(
    value: &T,
) -> Result<(), EncodingError> {
    let mut output = BoundedCounter {
        written: 0,
        limit: MAX_WORKFLOW_JSON_BYTES,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut output, value);
    if output.exceeded {
        return Err(EncodingError::Limit);
    }
    result.map_err(|_| EncodingError::Invalid)
}

struct BoundedCounter {
    written: usize,
    limit: usize,
    exceeded: bool,
}

impl Write for BoundedCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.written) {
            self.exceeded = true;
            return Err(std::io::Error::other("workflow JSON byte budget"));
        }
        self.written += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "workflow_serialization_tests.rs"]
mod tests;
