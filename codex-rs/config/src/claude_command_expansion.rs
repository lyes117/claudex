//! Single-pass expansion with a byte budget charged before each output append.
use serde_json::Value;
use std::io;
use std::path::Path;

pub(super) fn expand(
    metadata: &Value,
    body: &str,
    arguments: &str,
    tokens: &[String],
    path: &Path,
    max_bytes: usize,
) -> io::Result<String> {
    super::validate_skill(metadata)?;
    match metadata.get("user-invocable") {
        None | Some(Value::Null) | Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => {
            return Err(io::Error::other("Claude command is not user-invocable"));
        }
        Some(_) => {
            return Err(io::Error::other(
                "Invalid Claude command invocation metadata",
            ));
        }
    }
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
    let mut resolve = |captures: &regex_lite::Captures<'_>| {
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
    };
    let mut result = String::new();
    let mut end = 0;
    for captures in pattern.captures_iter(body) {
        let matched = captures.get(0).expect("complete match");
        append(&mut result, &body[end..matched.start()], max_bytes)?;
        append(&mut result, &resolve(&captures), max_bytes)?;
        end = matched.end();
    }
    append(&mut result, &body[end..], max_bytes)?;
    if !arguments.is_empty() && !substituted_argument {
        append(
            &mut result,
            &format!("\n\nArguments: {arguments}"),
            max_bytes,
        )?;
    }
    Ok(result)
}

fn append(output: &mut String, text: &str, max_bytes: usize) -> io::Result<()> {
    if output.len().saturating_add(text.len()) > max_bytes {
        return Err(io::Error::other(
            "Claude command exceeds the expanded text limit",
        ));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
#[path = "claude_command_expansion_tests.rs"]
mod tests;
