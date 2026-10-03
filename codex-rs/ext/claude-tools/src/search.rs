use codex_extension_api::FunctionCallError;
use codex_file_system::WalkEntryKind;
use codex_file_system::WalkOptions;
use codex_tools::ToolEnvironment;
use globset::Glob;
use regex::RegexBuilder;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::append_bounded;
use crate::error;
use crate::parse;
use crate::read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    output_mode: Option<String>,
    #[serde(rename = "-i")]
    ignore_case: Option<bool>,
    head_limit: Option<usize>,
    offset: Option<usize>,
}

pub(crate) async fn search(
    environment: &ToolEnvironment<'_>,
    arguments: &str,
    budget: usize,
    grep: bool,
) -> Result<Value, FunctionCallError> {
    let args: SearchArgs = parse(arguments)?;
    if args.pattern.len() > 1024
        || args.path.as_ref().is_some_and(|path| path.len() > 4096)
        || args.glob.as_ref().is_some_and(|glob| glob.len() > 1024)
    {
        return Err(error("Search path or pattern exceeds its size limit"));
    }
    if !grep && (args.glob.is_some() || args.output_mode.is_some() || args.ignore_case.is_some()) {
        return Err(error(
            "Glob accepts pattern, path, head_limit and offset only",
        ));
    }
    let mode = args.output_mode.as_deref().unwrap_or("files_with_matches");
    if grep && !matches!(mode, "content" | "files_with_matches" | "count") {
        return Err(error(
            "Grep output_mode must be content, files_with_matches or count",
        ));
    }
    let limit = args.head_limit.unwrap_or(200);
    if limit == 0 || limit > 200 || args.offset.unwrap_or(0) > 4000 {
        return Err(error(
            "Search head_limit must be between 1 and 200; offset may not exceed 4000",
        ));
    }
    let root = environment
        .cwd
        .join(args.path.as_deref().unwrap_or("."))
        .map_err(error)?;
    let sandbox = Some(&environment.file_system_sandbox_context);
    let metadata = environment
        .file_system
        .get_metadata(&root, Default::default(), sandbox)
        .await
        .map_err(error)?;
    let (mut paths, mut truncated, mut skipped) = if metadata.is_file {
        (vec![root.clone()], false, 0)
    } else if metadata.is_directory {
        let walk = environment
            .file_system
            .walk(
                &root,
                WalkOptions {
                    max_depth: 16,
                    max_directories: 1000,
                    max_entries: 4000,
                    follow_directory_symlinks: false,
                    prune_hidden_directories: false,
                },
                sandbox,
            )
            .await
            .map_err(error)?;
        (
            walk.entries
                .into_iter()
                .filter(|entry| entry.kind == WalkEntryKind::File)
                .map(|entry| entry.path)
                .collect(),
            walk.truncated,
            walk.errors.len(),
        )
    } else {
        return Err(error("Search requires a regular file or directory"));
    };
    paths.sort_by_key(ToString::to_string);
    let matcher = Glob::new(if grep {
        args.glob.as_deref().unwrap_or("**/*")
    } else {
        &args.pattern
    })
    .map_err(error)?
    .compile_matcher();
    let regex = if grep {
        Some(
            RegexBuilder::new(&args.pattern)
                .case_insensitive(args.ignore_case.unwrap_or(false))
                .size_limit(2 * 1024 * 1024)
                .dfa_size_limit(2 * 1024 * 1024)
                .build()
                .map_err(error)?,
        )
    } else {
        None
    };
    let mut rows = Vec::new();
    let mut matched = 0;
    let mut examined = 0;
    'files: for path in paths {
        let name = path
            .relative_path_from(&root)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.basename().unwrap_or_default());
        let name = name.replace('\\', "/");
        if !matcher.is_match(&name) {
            continue;
        }
        if let Some(regex) = &regex {
            if examined == 128 {
                truncated = true;
                break;
            }
            examined += 1;
            let contents = match read::text(environment, &path).await {
                Ok(contents) => contents,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let mut count = 0;
            for (index, line) in contents.lines().enumerate() {
                if index % 128 == 0 {
                    tokio::task::yield_now().await;
                }
                if !regex.is_match(line) {
                    continue;
                }
                count += 1;
                if mode != "content" {
                    continue;
                }
                matched += 1;
                if matched <= args.offset.unwrap_or(0) {
                    continue;
                }
                if rows.len() == limit
                    || !append_bounded(
                        &mut rows,
                        json!({"path": name, "line": index + 1, "text": line}),
                        budget,
                    )
                {
                    truncated = true;
                    break 'files;
                }
            }
            if mode == "content" {
                continue;
            }
            if count == 0 {
                continue;
            }
            matched += 1;
            if matched <= args.offset.unwrap_or(0) {
                continue;
            }
            let row = if mode == "count" {
                json!({"path": name, "count": count})
            } else {
                json!(name)
            };
            if rows.len() == limit || !append_bounded(&mut rows, row, budget) {
                truncated = true;
                break;
            }
        } else {
            matched += 1;
            if matched <= args.offset.unwrap_or(0) {
                continue;
            }
            if rows.len() == limit || !append_bounded(&mut rows, json!(name), budget) {
                truncated = true;
                break;
            }
        }
    }
    Ok(json!({"results": rows, "truncated": truncated, "skipped": skipped}))
}
