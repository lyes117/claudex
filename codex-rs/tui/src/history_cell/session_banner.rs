//! A compact, unboxed welcome for the classic terminal shell.
//! Session metadata reflects the native model and permissions, independently of branding.

use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::style::claudex_brand_color;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Clone, Copy)]
pub(super) struct BannerContent<'a> {
    pub version: &'a str,
    pub greeting: &'a str,
    pub model: &'a str,
    pub directory: &'a str,
    pub unrestricted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BannerLayout {
    Classic,
    Compact,
}

pub(super) fn render(content: BannerContent<'_>, width: u16) -> Vec<Line<'static>> {
    render_layout(content, width, BannerLayout::Classic)
}

pub(super) fn render_for_height(
    content: BannerContent<'_>,
    width: u16,
    available_rows: u16,
) -> Vec<Line<'static>> {
    let rows = usize::from(available_rows);
    if rows == 0 {
        return Vec::new();
    }
    let mut lines = render(content, width);
    if lines.len() > rows {
        lines = render_layout(content, width, BannerLayout::Compact);
        lines.truncate(rows);
    }
    lines
}

fn render_layout(
    content: BannerContent<'_>,
    width: u16,
    layout: BannerLayout,
) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 {
        return Vec::new();
    }
    let brand = claudex_brand_color();
    let mut metadata = vec![
        Line::from(vec![
            "Claudex".fg(brand).bold(),
            format!(" v{}", content.version).dim(),
            " · Codex".dim(),
        ]),
        Line::from(content.model.to_owned().bold()),
        Line::from(
            crate::text_formatting::center_truncate_path(
                content.directory,
                if layout == BannerLayout::Classic && width >= 32 {
                    width - 11
                } else {
                    width
                },
            )
            .dim(),
        ),
    ];
    if layout == BannerLayout::Classic && width >= 32 {
        // Original C mark. Keep identity separate from Claude Code's mascot and model.
        for (line, mark) in metadata
            .iter_mut()
            .zip(["  ▄████▄   ", " ██  ▄▄▄   ", "  ▀████▀   "])
        {
            line.spans.insert(0, mark.fg(brand));
        }
    }
    if content.unrestricted {
        metadata.push(Line::from(vec![
            "permissions: ".dim(),
            "YOLO mode".magenta().bold(),
        ]));
    }
    if layout == BannerLayout::Classic {
        metadata.push(Line::default());
        metadata.push(Line::from(content.greeting.to_owned().dim()));
    }
    let mut hints = vec!["/help".fg(brand), " commands · ".dim()];
    if width >= 72 {
        hints.extend(["/agents".fg(brand), " profiles · ".dim()]);
    }
    hints.extend(["/tasks".fg(brand), " agents".dim()]);
    if width >= 72 {
        hints.extend([" · ".dim(), "/workflows".fg(brand), " runs".dim()]);
    }
    metadata.push(Line::from(hints));
    metadata
        .into_iter()
        .map(|line| truncate_line_with_ellipsis_if_overflow(line, width))
        .collect()
}

#[cfg(test)]
#[path = "session_banner_tests.rs"]
mod tests;
