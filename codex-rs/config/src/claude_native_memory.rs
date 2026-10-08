//! Native memory takes precedence over the legacy Claude plugin without editing it.
use serde::Deserialize;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::Path;

const MAX_ACTIVATION_BYTES: u64 = 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Activation {
    version: u8,
    active: bool,
    plugin_id: String,
}

pub(super) fn active_for_user(user_home: &Path) -> io::Result<bool> {
    let marker = user_home.join(".claudex/memory/native-active.json");
    let source = match File::open(marker) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::with_capacity((MAX_ACTIVATION_BYTES + 1) as usize);
    source
        .take(MAX_ACTIVATION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ACTIVATION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native memory activation exceeds byte budget",
        ));
    }
    let activation: Activation = serde_json::from_slice(&bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid native memory activation",
        )
    })?;
    if activation.version != 1 || activation.plugin_id != "claude-mem@claudex-memory" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported native memory activation",
        ));
    }
    Ok(activation.active)
}

#[cfg(test)]
#[path = "claude_native_memory_tests.rs"]
mod tests;
