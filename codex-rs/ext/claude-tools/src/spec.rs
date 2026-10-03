use crate::FileTool;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) fn file_tool_spec(tool: FileTool) -> ToolSpec {
    let (name, description, properties, required) = match tool {
        FileTool::Read => (
            "Read",
            "Read a UTF-8 text file through the active Codex filesystem and sandbox. Returns numbered lines. Limit: 1 MiB file, 2000 requested lines, 8 KiB response. Binary/PDF/image files require another tool.",
            BTreeMap::from([
                (
                    "file_path".into(),
                    JsonSchema::string(Some("Absolute or cwd-relative file path.".into())),
                ),
                (
                    "offset".into(),
                    JsonSchema::integer(Some("First line, 1-based. Default 1.".into())),
                ),
                (
                    "limit".into(),
                    JsonSchema::integer(Some("Maximum lines, 1..2000. Default 2000.".into())),
                ),
            ]),
            "file_path",
        ),
        FileTool::Glob | FileTool::Grep => {
            let grep = matches!(tool, FileTool::Grep);
            let mut properties = BTreeMap::from([
                (
                    "pattern".into(),
                    JsonSchema::string(Some(
                        if grep {
                            "Rust regular expression, matched per line."
                        } else {
                            "Glob relative to path, e.g. **/*.rs."
                        }
                        .into(),
                    )),
                ),
                (
                    "path".into(),
                    JsonSchema::string(Some(
                        "Search root or file. Default cwd. No directory symlink traversal.".into(),
                    )),
                ),
                (
                    "head_limit".into(),
                    JsonSchema::integer(Some("Maximum results, 1..200. Default 200.".into())),
                ),
                (
                    "offset".into(),
                    JsonSchema::integer(Some("Results to skip, 0..4000. Default 0.".into())),
                ),
            ]);
            if grep {
                properties.insert(
                    "glob".into(),
                    JsonSchema::string(Some("Restrict relative file names with a glob.".into())),
                );
                properties.insert(
                    "output_mode".into(),
                    JsonSchema::string(Some(
                        "files_with_matches (default), content, or count (matching lines).".into(),
                    )),
                );
                properties.insert(
                    "-i".into(),
                    JsonSchema::boolean(Some("Case-insensitive regular expression.".into())),
                );
            }
            (
                if grep { "Grep" } else { "Glob" },
                if grep {
                    "Search UTF-8 text via the active Codex filesystem and sandbox. Bounded traversal: depth 16, 4000 entries, 128 files up to 1 MiB each; skipped files are counted. Rust regex syntax, single-line matches; 8 KiB response. Does not execute shell commands."
                } else {
                    "Find files via the active Codex filesystem and sandbox. Bounded traversal: depth 16, 4000 entries; 8 KiB response. Results sorted by relative path. Does not execute shell commands."
                },
                properties,
                "pattern",
            )
        }
    };
    ToolSpec::Function(ResponsesApiTool {
        name: name.into(),
        description: description.into(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec![required.into()]), Some(false.into())),
        output_schema: None,
    })
}
