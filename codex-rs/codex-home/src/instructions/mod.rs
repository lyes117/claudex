use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::Instructions;
use codex_extension_api::LoadInstructionsFuture;
use codex_extension_api::LoadedUserInstructions;
use codex_extension_api::UserInstructionsProvider;
use codex_utils_absolute_path::AbsolutePathBuf;

const DEFAULT_AGENTS_MD_FILENAME: &str = "AGENTS.md";
const LOCAL_AGENTS_MD_FILENAME: &str = "AGENTS.override.md";

/// Loads user instructions from a Codex home directory.
#[derive(Clone, Debug)]
pub struct CodexHomeUserInstructionsProvider {
    codex_home: AbsolutePathBuf,
    // Cached instructions and warning history are shared across provider clones.
    state: Arc<Mutex<InstructionsState>>,
}

#[derive(Debug, Default)]
struct InstructionsState {
    // Failed refreshes retain the last good value; confirmed absence still clears it.
    last_successful_instructions: Option<Instructions>,
    // Report ongoing failures once, then again if they return after recovery.
    active_warnings: HashSet<String>,
}

impl CodexHomeUserInstructionsProvider {
    /// Creates a provider rooted at the supplied absolute Codex home directory.
    pub fn new(codex_home: AbsolutePathBuf) -> Self {
        Self {
            codex_home,
            state: Arc::new(Mutex::new(InstructionsState::default())),
        }
    }

    #[tracing::instrument(name = "instructions.load", skip_all, fields(provider = "global"))]
    async fn load_from_codex_home(&self) -> LoadedUserInstructions {
        let mut warnings = Vec::new();
        for candidate in [LOCAL_AGENTS_MD_FILENAME, DEFAULT_AGENTS_MD_FILENAME] {
            let path = self.codex_home.join(candidate);
            match tokio::fs::metadata(path.as_path()).await {
                Ok(metadata) if !metadata.is_file() => continue,
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    warnings.push(format!(
                        "Failed to read global AGENTS.md instructions from `{}`: {err}",
                        path.display()
                    ));
                    continue;
                }
            }
            let data = match tokio::fs::read(path.as_path()).await {
                Ok(data) => data,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    warnings.push(format!(
                        "Failed to read global AGENTS.md instructions from `{}`: {err}",
                        path.display()
                    ));
                    continue;
                }
            };
            let contents = String::from_utf8_lossy(&data);
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                return LoadedUserInstructions {
                    instructions: Some(Instructions {
                        text: trimmed.to_string(),
                        source: Some(path),
                    }),
                    warnings,
                };
            }
        }
        LoadedUserInstructions {
            instructions: None,
            warnings,
        }
    }
}

impl UserInstructionsProvider for CodexHomeUserInstructionsProvider {
    fn load_user_instructions(&self) -> LoadInstructionsFuture<'_> {
        Box::pin(async move {
            let mut loaded = self.load_from_codex_home().await;
            if let Some(home) = self
                .codex_home
                .as_path()
                .parent()
                .filter(|_| {
                    self.codex_home
                        .as_path()
                        .file_name()
                        .is_some_and(|name| name == ".codex")
                })
                .map(|path| path.join(".claude"))
            {
                let mut paths = vec![home.join("CLAUDE.md")];
                let mut directories = vec![(home.join("rules"), 0)];
                while let Some((directory, depth)) = directories.pop() {
                    if depth > 6 || paths.len() >= 1000 {
                        break;
                    }
                    if let Ok(entries) = std::fs::read_dir(directory) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                directories.push((path, depth + 1));
                            } else if path.extension().is_some_and(|extension| extension == "md") {
                                paths.push(path);
                            }
                        }
                    }
                }
                paths.sort();
                let mut texts = Vec::new();
                let mut remaining = 32768;
                for path in paths {
                    if remaining == 0 {
                        break;
                    }
                    if let Ok(data) = tokio::fs::read(&path).await {
                        let data = &data[..data.len().min(remaining)];
                        remaining -= data.len();
                        let text = String::from_utf8_lossy(data);
                        texts.push(format!(
                            "# Claude instructions from {}\nApply any paths frontmatter only to matching files.\n{text}",
                            path.display()
                        ));
                    }
                }
                if !texts.is_empty() {
                    let text = texts.join("\n\n");
                    match loaded.instructions.as_mut() {
                        Some(instructions) => {
                            instructions.text.push_str("\n\n");
                            instructions.text.push_str(&text);
                        }
                        None => loaded.instructions = Some(Instructions { text, source: None }),
                    }
                }
            }
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if loaded.instructions.is_some() || loaded.warnings.is_empty() {
                state
                    .last_successful_instructions
                    .clone_from(&loaded.instructions);
            } else {
                loaded.instructions = state.last_successful_instructions.clone();
            }
            let previous_warnings = std::mem::take(&mut state.active_warnings);
            loaded.warnings.retain(|warning| {
                state.active_warnings.insert(warning.clone())
                    && !previous_warnings.contains(warning)
            });
            loaded
        })
    }
}

#[cfg(test)]
mod tests;
