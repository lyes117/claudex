//! The Claudex welcome shell. Branding does not imply a different model or permission mode.

use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::style::claudex_brand_color;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) struct BannerContent<'a> {
    pub version: &'a str,
    pub greeting: &'a str,
    pub model: &'a str,
    pub directory: &'a str,
    pub unrestricted: bool,
}

pub(super) fn render(content: BannerContent<'_>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).min(100);
    if width == 0 {
        return Vec::new();
    }
    let framed = width >= 4;
    let inner = if framed { width - 4 } else { width };
    let columns = inner >= 72;
    let left = if columns { inner - 31 } else { inner };
    let brand = claudex_brand_color();
    let mut lines = vec![
        Line::from(content.greeting.to_owned().fg(brand).bold()),
        Line::default(),
    ];
    // An original block C, rather than Claude Code's proprietary mascot.
    if inner >= 30 {
        for logo in ["     ▄████▄", "    ██  ▄▄▄", "     ▀████▀"]
        {
            lines.push(Line::from(logo.fg(brand)));
        }
        lines.push(Line::default());
    }
    lines.push(Line::from(vec![
        content.model.to_owned().bold(),
        " · Codex engine".dim(),
    ]));
    lines.push(Line::from(
        crate::text_formatting::center_truncate_path(content.directory, left).dim(),
    ));
    if columns {
        for (line, hint) in lines.iter_mut().zip([
            "Get started",
            "Describe a task to begin",
            "/help     commands",
            "/agents   profiles",
            "/tasks    session agents",
            "/workflows runs",
        ]) {
            *line = truncate_line_with_ellipsis_if_overflow(line.clone(), left);
            line.spans
                .push(" ".repeat(left.saturating_sub(line_width(line))).into());
            line.spans.push(" │ ".dim());
            line.spans.push(hint.to_owned().into());
        }
    } else {
        lines.push(Line::from(vec![
            "/help".fg(brand),
            " commands · ".dim(),
            "/tasks".fg(brand),
            " agents".dim(),
        ]));
    }
    if content.unrestricted {
        lines.push(Line::from(vec![
            "permissions: ".dim(),
            "YOLO mode".magenta().bold(),
        ]));
    }
    let lines: Vec<_> = lines
        .into_iter()
        .map(|line| truncate_line_with_ellipsis_if_overflow(line, inner))
        .collect();
    if !framed {
        return lines;
    }
    let title = truncate_line_with_ellipsis_if_overflow(
        Line::from(format!(" Claudex v{} ", content.version).fg(brand)),
        width - 2,
    );
    let padding = (width - 2).saturating_sub(line_width(&title));
    let mut top = vec!["╭".fg(brand)];
    top.extend(title.spans);
    top.push("─".repeat(padding).fg(brand));
    top.push("╮".fg(brand));
    let mut output = vec![Line::from(top)];
    for line in lines {
        let used = line_width(&line);
        let mut spans = vec!["│ ".fg(brand)];
        spans.extend(line.spans);
        spans.push(" ".repeat(inner.saturating_sub(used)).into());
        spans.push(" │".fg(brand));
        output.push(Line::from(spans));
    }
    output.push(Line::from(format!("╰{}╯", "─".repeat(width - 2)).fg(brand)));
    output
}

#[cfg(test)]
#[path = "session_banner_tests.rs"]
mod tests;
