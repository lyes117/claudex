use codex_extension_api::FunctionCallError;
use codex_tools::ToolEnvironment;
use codex_utils_path_uri::PathUri;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::MAX_FILE_BYTES;
use crate::append_bounded;
use crate::error;
use crate::parse;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    file_path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(crate) async fn text(
    environment: &ToolEnvironment<'_>,
    path: &PathUri,
) -> Result<String, FunctionCallError> {
    let sandbox = Some(&environment.file_system_sandbox_context);
    let metadata = environment
        .file_system
        .get_metadata(path, Default::default(), sandbox)
        .await
        .map_err(error)?;
    if !metadata.is_file || metadata.size > MAX_FILE_BYTES as u64 {
        return Err(error(
            "Read requires a regular UTF-8 text file no larger than 1 MiB",
        ));
    }
    let mut stream = environment
        .file_system
        .read_file_stream(path, sandbox)
        .await
        .map_err(error)?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_FILE_BYTES {
            return Err(error("File grew beyond the 1 MiB read limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.contains(&0) {
        return Err(error("Binary files require a different tool"));
    }
    String::from_utf8(bytes).map_err(|_| {
        error("File is not valid UTF-8; PDF and image reads are not supported by this tool")
    })
}

pub(crate) async fn read(
    environment: &ToolEnvironment<'_>,
    arguments: &str,
    budget: usize,
) -> Result<Value, FunctionCallError> {
    let args: ReadArgs = parse(arguments)?;
    let offset = args.offset.unwrap_or(1);
    let limit = args.limit.unwrap_or(2000);
    if args.file_path.len() > 4096 || offset == 0 || limit == 0 || limit > 2000 {
        return Err(error(
            "Read requires a bounded file path, a 1-based offset and limit between 1 and 2000",
        ));
    }
    let path = environment.cwd.join(&args.file_path).map_err(error)?;
    let contents = text(environment, &path).await?;
    let mut rows = Vec::new();
    let mut truncated = false;
    for (index, line) in contents.lines().enumerate().skip(offset - 1) {
        if rows.len() == limit
            || !append_bounded(&mut rows, json!({"line": index + 1, "text": line}), budget)
        {
            truncated = true;
            break;
        }
    }
    Ok(json!({"lines": rows, "truncated": truncated, "offset": offset}))
}
