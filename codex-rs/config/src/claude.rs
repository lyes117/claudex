//! Read Claude's documented configuration in place. Never migrate source files.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;
use toml::Value as TomlValue;

pub fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|path| path.join(".claude"))
}

pub fn directory_for_layer(layer: &crate::ConfigLayerEntry) -> Option<PathBuf> {
    if layer.is_disabled() || !layer.claude_config_enabled {
        return None;
    }
    match &layer.name {
        crate::ConfigLayerSource::User { .. } => layer
            .config_folder()?
            .parent()
            .map(|p| p.join(".claude").into_path_buf()),
        crate::ConfigLayerSource::Project { .. } => layer
            .config_folder()?
            .parent()
            .map(|p| p.join(".claude").into_path_buf()),
        _ => None,
    }
}

pub fn active_directory<'a>(
    layers: impl Iterator<Item = &'a crate::ConfigLayerEntry>,
) -> Option<PathBuf> {
    layers
        .filter(|layer| !layer.is_disabled())
        .filter_map(directory_for_layer)
        .filter(|directory| directory.is_dir())
        .last()
}

pub fn user_config_enabled<'a>(
    mut layers: impl Iterator<Item = &'a crate::ConfigLayerEntry>,
) -> bool {
    layers.any(|layer| {
        matches!(layer.name, crate::ConfigLayerSource::User { .. })
            && directory_for_layer(layer).is_some()
    })
}

pub fn read_json(path: &Path) -> io::Result<Value> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid JSON in {}", path.display()),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(error),
    }
}

pub fn has_project_mcp(directory: &Path) -> io::Result<bool> {
    let Some(home) = home().and_then(|home| home.parent().map(Path::to_path_buf)) else {
        return Ok(false);
    };
    let source = read_json(&home.join(".claude.json"))?;
    Ok(source
        .get("projects")
        .and_then(Value::as_object)
        .is_some_and(|projects| {
            projects.iter().any(|(path, settings)| {
                Path::new(path) == directory
                    && settings
                        .get("mcpServers")
                        .and_then(Value::as_object)
                        .is_some_and(|servers| !servers.is_empty())
            })
        }))
}

pub fn settings(directory: &Path) -> io::Result<Value> {
    let mut result = read_json(&directory.join("settings.json"))?;
    merge_settings(
        &mut result,
        read_json(&directory.join("settings.local.json"))?,
    );
    Ok(result)
}

fn merge_settings(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                merge_settings(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (Value::Array(base), Value::Array(overlay)) => {
            for value in overlay {
                if !base.contains(&value) {
                    base.push(value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn merge_json(base: &mut Value, overlay: Value) {
    if let (Some(base), Some(overlay)) = (base.as_object_mut(), overlay.as_object()) {
        for (key, value) in overlay {
            if value.is_object() && base.get(key).is_some_and(Value::is_object) {
                if let Some(previous) = base.get_mut(key) {
                    merge_json(previous, value.clone());
                }
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Metadata and body for skills, commands and agent Markdown files.
pub fn is_markdown_source(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == ".claude")
        || path
            .ancestors()
            .any(|root| root.join(".claude-plugin/plugin.json").is_file())
}

pub fn markdown(contents: &str) -> io::Result<(Value, String)> {
    let normalized = contents
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    if let Some(rest) = normalized.strip_prefix("---\n") {
        let (yaml, body) = rest.split_once("\n---").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Unterminated Markdown frontmatter",
            )
        })?;
        let metadata: Value = serde_yaml::from_str(yaml).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid Markdown frontmatter")
        })?;
        if !metadata.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Markdown frontmatter must be a mapping",
            ));
        }
        return Ok((metadata, body.trim_start_matches('\n').to_owned()));
    }
    Ok((serde_json::json!({}), normalized))
}

/// Resolve only locally installed, explicitly enabled plugins. No downloads.
pub fn plugins(directory: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    plugins_for_scope(directory, true)
}

pub fn plugins_for_scope(
    directory: &Path,
    include_user: bool,
) -> io::Result<Vec<(String, PathBuf)>> {
    let Some(home) = home() else {
        return Ok(Vec::new());
    };
    let mut merged = if include_user {
        settings(&home)?
    } else {
        serde_json::json!({})
    };
    if directory != home {
        let mut directories = Vec::new();
        if let Some(parent) = directory.parent() {
            for ancestor in parent.ancestors() {
                directories.push(ancestor.join(".claude"));
                if ancestor.join(".git").exists() {
                    break;
                }
            }
        }
        directories.reverse();
        for directory in directories {
            if !include_user && directory == home {
                continue;
            }
            merge_settings(&mut merged, settings(&directory)?);
        }
    }
    let installed = read_json(&home.join("plugins/installed_plugins.json"))?;
    let mut roots = Vec::new();
    if let Some(enabled) = merged.get("enabledPlugins").and_then(Value::as_object) {
        for (name, enabled) in enabled {
            if enabled != &Value::Bool(true) {
                continue;
            }
            let Some(entries) = installed
                .get("plugins")
                .and_then(|p| p.get(name))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for entry in entries {
                if let Some(project) = entry.get("projectPath").and_then(Value::as_str)
                    && !directory.starts_with(project)
                {
                    continue;
                }
                if let Some(path) = entry.get("installPath").and_then(Value::as_str)
                    && Path::new(path).is_dir()
                {
                    roots.push((name.clone(), PathBuf::from(path)));
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn expand_environment(value: &str, plugin_root: Option<&Path>) -> io::Result<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some((prefix, tail)) = rest.split_once("${") {
        result.push_str(prefix);
        let (variable, tail) = tail.split_once('}').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Unterminated MCP environment reference",
            )
        })?;
        let (name, default) = variable
            .split_once(":-")
            .map_or((variable, None), |(name, default)| (name, Some(default)));
        let expanded = if name == "CLAUDE_PLUGIN_ROOT" {
            plugin_root.map(|p| p.to_string_lossy().into_owned())
        } else {
            std::env::var(name).ok()
        }
        .or_else(|| default.map(str::to_owned))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Missing MCP environment variable {name}"),
            )
        })?;
        result.push_str(&expanded);
        rest = tail;
    }
    result.push_str(rest);
    Ok(result)
}

fn expand_json(value: &mut Value, root: Option<&Path>) -> io::Result<()> {
    match value {
        Value::String(text) => *text = expand_environment(text, root)?,
        Value::Array(values) => {
            for value in values {
                expand_json(value, root)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                expand_json(value, root)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn mcp_config(path: &Path, root: Option<&Path>, warnings: &mut Vec<String>) -> io::Result<Value> {
    let source = read_json(path)?;
    mcp_value(&source, path, root, warnings)
}

fn mcp_value(
    source: &Value,
    path: &Path,
    root: Option<&Path>,
    warnings: &mut Vec<String>,
) -> io::Result<Value> {
    let servers = source.get("mcpServers").unwrap_or(source);
    let mut output = serde_json::Map::new();
    if let Some(servers) = servers.as_object() {
        for (name, server) in servers {
            if !server.is_object() {
                continue;
            }
            let mut server = server.clone();
            if let Err(error) = expand_json(&mut server, root) {
                warnings.push(format!(
                    "Claudex: MCP {name} in {} disabled: {error}",
                    path.display()
                ));
                continue;
            }
            let mut mapped = serde_json::Map::new();
            match server
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("stdio")
            {
                "stdio" => {
                    for key in ["command", "args", "env", "cwd"] {
                        if let Some(value) = server.get(key) {
                            mapped.insert(key.into(), value.clone());
                        }
                    }
                }
                "http" => {
                    for (source, target) in [("url", "url"), ("headers", "http_headers")] {
                        if let Some(value) = server.get(source) {
                            mapped.insert(target.into(), value.clone());
                        }
                    }
                }
                transport => {
                    warnings.push(format!(
                        "Claudex: MCP transport {transport} in {} is unsupported",
                        path.display()
                    ));
                    continue;
                }
            }
            if !mapped.contains_key("command") && !mapped.contains_key("url") {
                continue;
            }
            output.insert(name.clone(), Value::Object(mapped));
        }
    }
    Ok(Value::Object(output))
}

/// Lower-priority native configuration for one Claude scope.
pub fn native_config(directory: &Path, warnings: &mut Vec<String>) -> io::Result<TomlValue> {
    let settings = settings(directory)?;
    let mut native = serde_json::json!({});
    if settings.get("disableAllHooks") == Some(&Value::Bool(true)) {
        native["features"] = serde_json::json!({"hooks":false});
    }
    if let Some(env) = settings.get("env") {
        native["shell_environment_policy"] = serde_json::json!({"set": env});
    }
    let mut servers = serde_json::json!({});
    if let Some(home) = home().and_then(|home| home.parent().map(Path::to_path_buf)) {
        let path = home.join(".claude.json");
        let source = read_json(&path)?;
        if directory == home.join(".claude") {
            if let Some(global) = source.get("mcpServers") {
                merge_json(&mut servers, mcp_value(global, &path, None, warnings)?);
            }
        } else if let Some(projects) = source.get("projects").and_then(Value::as_object) {
            for (project, config) in projects {
                if directory
                    .parent()
                    .is_some_and(|parent| parent == Path::new(project))
                    && let Some(project) = config.get("mcpServers")
                {
                    merge_json(&mut servers, mcp_value(project, &path, None, warnings)?);
                }
            }
        }
    }
    if let Some(parent) = directory.parent() {
        merge_json(
            &mut servers,
            mcp_config(&parent.join(".mcp.json"), None, warnings)?,
        );
    }
    if servers.as_object().is_some_and(|s| !s.is_empty()) {
        native["mcp_servers"] = servers;
    }
    if let Some(disabled) = settings
        .get("disabledMcpjsonServers")
        .and_then(Value::as_array)
    {
        for name in disabled.iter().filter_map(Value::as_str) {
            if let Some(server) = native
                .get_mut("mcp_servers")
                .and_then(|servers| servers.get_mut(name))
            {
                server["enabled"] = Value::Bool(false);
            }
        }
    }
    // Claude model names and API credentials are intentionally never mapped.
    TomlValue::try_from(native).map_err(io::Error::other)
}

pub fn plugin_config(
    directory: &Path,
    include_user: bool,
    warnings: &mut Vec<String>,
) -> io::Result<TomlValue> {
    let mut servers = serde_json::json!({});
    for (_, root) in plugins_for_scope(directory, include_user)? {
        merge_json(
            &mut servers,
            mcp_config(&root.join(".mcp.json"), Some(&root), warnings)?,
        );
    }
    if servers
        .as_object()
        .is_some_and(|servers| !servers.is_empty())
    {
        TomlValue::try_from(serde_json::json!({"mcp_servers": servers})).map_err(io::Error::other)
    } else {
        Ok(TomlValue::Table(toml::map::Map::new()))
    }
}

/// Fail closed on deny/ask rules. Allow rules never relax Codex's own policies.
pub fn permission_block(
    stack: &crate::ConfigLayerStack,
    tool: &str,
    input: &Value,
) -> io::Result<Option<String>> {
    let directories = stack
        .layers_low_to_high()
        .filter(|layer| !layer.is_disabled())
        .filter_map(directory_for_layer)
        .collect::<Vec<_>>();
    permission_block_in_directories(&directories, tool, input)
}

/// Reject execution metadata whose constraints cannot be represented natively.
pub fn validate_skill(metadata: &Value) -> io::Result<()> {
    for field in ["disallowed-tools", "hooks", "context", "agent"] {
        if metadata.get(field).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("Claudex cannot enforce skill `{field}` metadata; skill not loaded"),
            ));
        }
    }
    Ok(())
}

pub fn expand_command(
    metadata: &Value,
    body: &str,
    arguments: &str,
    tokens: &[String],
    path: &Path,
) -> io::Result<String> {
    validate_skill(metadata)?;
    if body.contains("!`") || body.contains("${CLAUDE_PROJECT_DIR}") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Dynamic shell injection and session project-dir interpolation are unsupported",
        ));
    }
    let names: Vec<&str> = match metadata.get("arguments") {
        Some(Value::String(names)) => names.split_whitespace().collect(),
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let pattern = regex_lite::Regex::new(r"\\?\$(?:ARGUMENTS(?:\[\d+\])?|\d+|\{CLAUDE_(?:SKILL|PROJECT)_DIR\}|[a-zA-Z_][a-zA-Z_0-9]*)").map_err(io::Error::other)?;
    let mut substituted_argument = false;
    let result = pattern.replace_all(body, |captures: &regex_lite::Captures<'_>| {
        let original = &captures[0];
        if let Some(literal) = original.strip_prefix('\\') {
            return literal.to_owned();
        }
        let name = &original[1..];
        if name == "ARGUMENTS" {
            substituted_argument = true;
            return arguments.to_owned();
        }
        if let Ok(index) = name
            .strip_prefix("ARGUMENTS[")
            .and_then(|index| index.strip_suffix(']'))
            .unwrap_or(name)
            .parse::<usize>()
        {
            substituted_argument = true;
            return tokens
                .get(index)
                .cloned()
                .unwrap_or_else(|| original.to_owned());
        }
        if let Some(index) = names.iter().position(|candidate| *candidate == name) {
            substituted_argument = true;
            return tokens.get(index).cloned().unwrap_or_default();
        }
        if name == "{CLAUDE_SKILL_DIR}" {
            return path
                .parent()
                .unwrap_or(Path::new("."))
                .display()
                .to_string();
        }
        original.to_owned()
    });
    let mut result = result.into_owned();
    if !arguments.is_empty() && !substituted_argument {
        result.push_str(&format!("\n\nArguments: {arguments}"));
    }
    Ok(result)
}

fn permission_block_in_directories(
    directories: &[PathBuf],
    tool: &str,
    input: &Value,
) -> io::Result<Option<String>> {
    for directory in directories {
        let settings = settings(directory)?;
        for kind in ["deny", "ask"] {
            for rule in settings
                .pointer(&format!("/permissions/{kind}"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let (name, argument) = rule
                    .split_once('(')
                    .map_or((rule, None), |(name, argument)| {
                        (name, Some(argument.trim_end_matches(')')))
                    });
                let shell = matches!(
                    tool,
                    "Bash" | "shell" | "shell_command" | "exec_command" | "write_stdin"
                );
                let applies = match name {
                    "Bash" | "PowerShell" => shell,
                    "Edit" | "Write" => tool == "apply_patch" || shell,
                    "Read" | "Glob" | "Grep" => {
                        shell
                            || matches!(
                                tool,
                                "view_image" | "read_file" | "list_dir" | "grep_files"
                            )
                    }
                    "Agent" | "Task" => tool == "spawn_agent",
                    "WebFetch" | "WebSearch" => tool.contains("web"),
                    _ => wildmatch::WildMatch::new(name).matches(tool),
                };
                if !applies {
                    continue;
                }
                let matches = if matches!(name, "Bash" | "PowerShell") {
                    let command = input
                        .get("command")
                        .or_else(|| input.get("cmd"))
                        .and_then(Value::as_str);
                    match (argument, command) {
                        (None, _) => true,
                        (Some(pattern), Some(command)) => {
                            let pattern = format!("*{}*", pattern.replace(":*", "*"));
                            wildmatch::WildMatch::new(&pattern.to_lowercase())
                                .matches(&command.to_lowercase())
                        }
                        // An opaque shell continuation cannot be classified safely.
                        (Some(_), None) => true,
                    }
                } else {
                    true
                };
                if matches {
                    return Ok(Some(format!(
                        "Claudex: tool blocked by Claude {kind} permission in {}. Path-scoped file rules conservatively block the entire tool; ask rules require an explicit configuration change.",
                        directory.display()
                    )));
                }
            }
        }
    }
    Ok(None)
}

pub fn agent_config(
    contents: &str,
    path: &Path,
) -> io::Result<(String, Option<String>, TomlValue)> {
    let (metadata, body) = markdown(contents)?;
    for field in [
        "tools",
        "disallowedTools",
        "permissionMode",
        "hooks",
        "isolation",
    ] {
        if metadata.get(field).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "Claudex cannot enforce agent `{field}` metadata in {}; agent not loaded",
                    path.display()
                ),
            ));
        }
    }
    let name = metadata
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Agent has no name"))?;
    let description = metadata
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut config = BTreeMap::new();
    config.insert("developer_instructions", TomlValue::String(body));
    Ok((
        name,
        description,
        TomlValue::try_from(config).map_err(io::Error::other)?,
    ))
}

pub fn hook_sources(
    directory: &Path,
    warnings: &mut Vec<String>,
) -> io::Result<Vec<(PathBuf, crate::HookEventsToml, Option<PathBuf>)>> {
    hook_sources_for_scope(directory, Some(directory), true, warnings)
}

pub fn hook_sources_for_scope(
    directory: &Path,
    plugin_directory: Option<&Path>,
    include_user: bool,
    warnings: &mut Vec<String>,
) -> io::Result<Vec<(PathBuf, crate::HookEventsToml, Option<PathBuf>)>> {
    if settings(directory)?.get("disableAllHooks") == Some(&Value::Bool(true)) {
        return Ok(Vec::new());
    }
    let mut sources = vec![
        (directory.join("settings.json"), None),
        (directory.join("settings.local.json"), None),
    ];
    if plugin_directory == Some(directory) {
        for (_, root) in plugins_for_scope(directory, include_user)? {
            sources.push((root.join("hooks/hooks.json"), Some(root)));
        }
    }
    let mut output = Vec::new();
    for (path, root) in sources {
        let source = read_json(&path)?;
        let Some(hooks) = source.get("hooks").and_then(Value::as_object) else {
            continue;
        };
        let mut supported = serde_json::Map::new();
        for (event, groups) in hooks {
            if ![
                "PreToolUse",
                "PermissionRequest",
                "PostToolUse",
                "SessionStart",
                "SessionEnd",
                "UserPromptSubmit",
                "SubagentStart",
                "SubagentStop",
                "Stop",
                "Interrupt",
                "PreCompact",
                "PostCompact",
            ]
            .contains(&event.as_str())
            {
                warnings.push(format!(
                    "Claudex: hook event {event} in {} is unsupported",
                    path.display()
                ));
                continue;
            }
            let mut groups = groups.clone();
            if let Some(groups) = groups.as_array_mut() {
                for group in groups {
                    if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                        handlers.retain(|handler| {
                            if matches!(
                                handler.get("type").and_then(Value::as_str),
                                Some("command" | "mcp_tool" | "prompt" | "agent")
                            ) {
                                true
                            } else {
                                warnings.push(format!(
                                    "Claudex: unsupported {event} hook handler in {} skipped",
                                    path.display()
                                ));
                                false
                            }
                        });
                    }
                }
            }
            supported.insert(event.clone(), groups);
        }
        match serde_json::from_value(Value::Object(supported)) {
            Ok(hooks) => output.push((path, hooks, root)),
            Err(_) => warnings.push(format!(
                "Claudex: unsupported hook handler in {}; supported types: command, mcp_tool",
                path.display()
            )),
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
