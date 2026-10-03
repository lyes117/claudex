//! Dynamic tool projections keep truthful status and bounded previews without losing detail.

use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use crate::thread_transcript::RawReasoningVisibility;
use crate::thread_transcript::thread_items_to_transcript_cells;
use pretty_assertions::assert_eq;
use serde_json::json;

fn item(status: DynamicToolCallStatus, output: Option<&str>) -> ThreadItem {
    ThreadItem::DynamicToolCall {
        id: "dynamic-1".to_string(),
        namespace: Some("example".to_string()),
        tool: "inspect".to_string(),
        arguments: json!({"path": "src/main.rs"}),
        status,
        content_items: output.map(|text| {
            vec![DynamicToolCallOutputContentItem::InputText {
                text: text.to_string(),
            }]
        }),
        success: None,
        duration_ms: None,
    }
}

#[test]
fn dynamic_status_and_output_match_persisted_presentations() {
    let cwd = test_path_buf("/workspace").abs();
    let mut snapshots = Vec::new();
    for (label, status, output) in [
        ("pending", DynamicToolCallStatus::InProgress, None),
        (
            "success",
            DynamicToolCallStatus::Completed,
            Some("Found the definition"),
        ),
        (
            "failure",
            DynamicToolCallStatus::Failed,
            Some("Permission denied"),
        ),
        ("unavailable", DynamicToolCallStatus::Completed, None),
    ] {
        let replayed = thread_items_to_transcript_cells(
            /*thread_id*/ None,
            &cwd,
            [item(status, output)],
            RawReasoningVisibility::Hidden,
            /*config*/ None,
        );
        assert_eq!(replayed.len(), 1);
        let cell = &replayed[0];
        for (mode, lines) in [
            ("compact", cell.display_lines(/*width*/ 80)),
            (
                "full",
                visible_lines(cell.transcript_hyperlink_lines(/*width*/ 80)),
            ),
            ("raw", cell.raw_lines()),
        ] {
            let text = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            if mode == "compact" || label == "success" {
                snapshots.push(format!("{label}, {mode}\n{text}"));
            }
        }
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn claude_file_tool_cards_render_live_and_restored_without_losing_output() {
    let cwd = test_path_buf("/workspace").abs();
    let mut snapshots = Vec::new();
    for (tool, arguments, output) in [
        (
            "Read",
            json!({"file_path":"src/main.rs"}),
            json!({"lines":[{"line":1,"text":"premier été"}],"offset":1,"truncated":false}),
        ),
        (
            "Glob",
            json!({"pattern":"**/*.rs"}),
            json!({"results":["src/main.rs"],"skipped":0,"truncated":false}),
        ),
        (
            "Grep",
            json!({"pattern":"main","output_mode":"content"}),
            json!({"results":[{"path":"src/main.rs","line":1,"text":"fn main() {}"}],"skipped":0,"truncated":false}),
        ),
    ] {
        let output = output.to_string();
        let item = ThreadItem::DynamicToolCall {
            id: tool.to_owned(),
            namespace: None,
            tool: tool.to_owned(),
            arguments,
            status: DynamicToolCallStatus::Completed,
            content_items: Some(vec![DynamicToolCallOutputContentItem::InputText {
                text: output.clone(),
            }]),
            success: Some(true),
            duration_ms: Some(25),
        };
        let live = DynamicToolCallCell::from_item(item.clone()).unwrap();
        let restored = thread_items_to_transcript_cells(
            /*thread_id*/ None,
            &cwd,
            [item],
            RawReasoningVisibility::Hidden,
            /*config*/ None,
        );
        assert_eq!(restored.len(), 1);
        for width in [26, 80] {
            let lines = live.display_lines(width);
            assert_eq!(lines, restored[0].display_lines(width));
            assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
            snapshots.push(format!(
                "{tool}, width={width}\n{}",
                lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let raw = live
            .raw_lines()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(raw.ends_with(&output));
        snapshots.push(format!("{tool}, full retained output\n{raw}"));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn dynamic_preview_reports_hidden_lines_and_retains_full_output() {
    let mut snapshots = Vec::new();
    for count in [3, 4] {
        let output = (1..=count)
            .map(|index| format!("Result line {index}: retained content"))
            .collect::<Vec<_>>()
            .join("\n");
        let cell =
            DynamicToolCallCell::from_item(item(DynamicToolCallStatus::Completed, Some(&output)))
                .unwrap();
        for width in [20, 80] {
            let compact = cell.display_lines(width);
            assert!(
                compact
                    .iter()
                    .all(|line| line.width() <= usize::from(width))
            );
            let text = compact
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            snapshots.push(format!("lines={count}, width={width}\n{text}"));
        }
        let raw = cell
            .raw_lines()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(raw.ends_with(&output));
        let detailed = visible_lines(cell.transcript_hyperlink_lines(/*width*/ 80))
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(output.lines().all(|line| detailed.contains(line)));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}
