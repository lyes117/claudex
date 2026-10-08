//! Bounded hook argv expansion. Event values never become shell source.

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 65_536;
const MAX_EVENT_BYTES: usize = 1_048_576;

pub(super) fn expand_args(
    program: &str,
    args: &[String],
    event_json: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> Result<Vec<String>, ()> {
    if program.is_empty()
        || program.len() > 4096
        || program.contains('\0')
        || args.len() > MAX_ARGS
        || event_json.len() > MAX_EVENT_BYTES
    {
        return Err(());
    }
    // Windows invokes batch files through cmd.exe even when using Command::new.
    // Keep them on the explicit legacy shell path rather than claiming literal argv.
    #[cfg(windows)]
    if Path::new(program)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
    {
        return Err(());
    }
    validate_args(args)?;
    let event: Value = serde_json::from_str(event_json).map_err(|_| ())?;
    let pattern = Regex::new(r"\$\{([^{}]+)\}").map_err(|_| ())?;
    let mut bytes = 0_usize;
    let mut expanded = Vec::with_capacity(args.len());
    for arg in args {
        let mut output = String::new();
        let mut end = 0;
        for capture in pattern.captures_iter(arg) {
            let placeholder = capture.get(0).ok_or(())?;
            push_bounded(&mut output, &arg[end..placeholder.start()], &mut bytes)?;
            if &capture[1] == "CLAUDE_PROJECT_DIR" {
                push_bounded(&mut output, &cwd.to_string_lossy(), &mut bytes)?;
            } else if let Some(value) = env.get(&capture[1]) {
                push_bounded(&mut output, value, &mut bytes)?;
            } else {
                let value = capture[1]
                    .split('.')
                    .try_fold(&event, |value, key| value.get(key).ok_or(()))?;
                push_bounded(&mut output, value.as_str().ok_or(())?, &mut bytes)?;
            }
            end = placeholder.end();
        }
        push_bounded(&mut output, &arg[end..], &mut bytes)?;
        expanded.push(output);
    }
    Ok(expanded)
}

fn push_bounded(output: &mut String, value: &str, bytes: &mut usize) -> Result<(), ()> {
    let next = bytes.checked_add(value.len()).ok_or(())?;
    if next > MAX_ARG_BYTES || value.contains('\0') {
        return Err(());
    }
    output.push_str(value);
    *bytes = next;
    Ok(())
}

pub(super) fn validate_args(args: &[String]) -> Result<(), ()> {
    if args.len() > MAX_ARGS {
        return Err(());
    }
    let mut bytes = 0_usize;
    for arg in args {
        bytes = bytes.checked_add(arg.len()).ok_or(())?;
        if bytes > MAX_ARG_BYTES || arg.contains('\0') {
            return Err(());
        }
    }
    Ok(())
}

/// Resolve only discovery-owned environment values, once and before allocation.
pub(super) fn expand_program(program: &str, env: &HashMap<String, String>) -> Result<String, ()> {
    if program.is_empty() || program.len() > 4096 || program.contains('\0') {
        return Err(());
    }
    let pattern = Regex::new(r"\$\{([^{}]+)\}").map_err(|_| ())?;
    let mut output = String::new();
    let mut end = 0;
    for capture in pattern.captures_iter(program) {
        let placeholder = capture.get(0).ok_or(())?;
        for value in [
            &program[end..placeholder.start()],
            env.get(&capture[1]).ok_or(())?.as_str(),
        ] {
            let next = output.len().checked_add(value.len()).ok_or(())?;
            if next > 4096 || value.contains('\0') {
                return Err(());
            }
            output.push_str(value);
        }
        end = placeholder.end();
    }
    let rest = &program[end..];
    if output.len().checked_add(rest.len()).ok_or(())? > 4096 {
        return Err(());
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
#[path = "command_args_tests.rs"]
mod tests;
