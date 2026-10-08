//! Bounded visual projection of actual native update_plan events.
//!
//! The complete event is still retained by native history. Visibility is local to
//! this thread's widget and does not affect plan execution or model context.

use super::ChatWidget;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableItem;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use crossterm::cursor::SetCursorStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const MAX_STEPS: usize = 32;
const MAX_STEP_CHARS: usize = 512;
const MAX_PANEL_ROWS: u16 = 6;

struct DisplayStep {
    text: String,
    status: StepStatus,
}

struct Snapshot {
    steps: Vec<DisplayStep>,
    total: usize,
    completed: usize,
}

#[derive(Default)]
pub(super) struct PlanChecklist {
    pub(super) visible: bool,
    snapshot: Option<Snapshot>,
}

impl PlanChecklist {
    pub(super) fn update(&mut self, update: &UpdatePlanArgs) {
        self.snapshot = Some(Snapshot {
            total: update.plan.len(),
            completed: update
                .plan
                .iter()
                .filter(|step| matches!(step.status, StepStatus::Completed))
                .count(),
            steps: update
                .plan
                .iter()
                .take(MAX_STEPS)
                .map(|step| DisplayStep {
                    text: display_step(&step.step),
                    status: step.status.clone(),
                })
                .collect(),
        });
    }

    fn desired_height(&self) -> u16 {
        self.snapshot.as_ref().map_or(2, |snapshot| {
            u16::try_from(snapshot.total.saturating_add(1))
                .unwrap_or(u16::MAX)
                .clamp(2, MAX_PANEL_ROWS)
        })
    }

    fn lines(&self, rows: u16) -> Vec<Line<'static>> {
        let Some(snapshot) = &self.snapshot else {
            return vec![
                "Tasks".bold().into(),
                "No native plan received for this session".dim().into(),
            ];
        };
        let mut lines = vec![
            format!("Tasks · {}/{} complete", snapshot.completed, snapshot.total)
                .bold()
                .into(),
        ];
        if snapshot.total == 0 {
            lines.push("No steps in the latest plan".dim().into());
            return lines;
        }
        let available = usize::from(rows.saturating_sub(1));
        let shown = if snapshot.total > available {
            available.saturating_sub(1)
        } else {
            available
        };
        for step in snapshot.steps.iter().take(shown) {
            let marker = match step.status {
                StepStatus::Completed => "✓ ".green(),
                StepStatus::InProgress => "◐ ".bold(),
                StepStatus::Pending => "○ ".dim(),
            };
            lines.push(vec![marker, step.text.clone().into()].into());
        }
        let omitted = snapshot
            .total
            .saturating_sub(shown.min(snapshot.steps.len()));
        if omitted > 0 {
            lines.push(
                format!("… {omitted} other steps · full plan in transcript")
                    .dim()
                    .into(),
            );
        }
        lines
    }
}

fn display_step(text: &str) -> String {
    let mut output = String::new();
    for (index, character) in text.chars().take(MAX_STEP_CHARS + 1).enumerate() {
        if index == MAX_STEP_CHARS {
            output.push('…');
            break;
        }
        output.push(
            if character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                ' '
            } else {
                character
            },
        );
    }
    output
}

impl ChatWidget {
    pub(crate) fn toggle_plan_checklist(&mut self) {
        self.transcript.plan_checklist.visible = !self.transcript.plan_checklist.visible;
        self.request_redraw();
    }
}

/// Reserve the original composer/footer first, then spend only spare rows on tasks.
pub(super) struct ChecklistComposition<'a> {
    pub(super) checklist: &'a PlanChecklist,
    pub(super) child: RenderableItem<'a>,
}

impl ChecklistComposition<'_> {
    fn panel_height(&self, area: Rect) -> u16 {
        let spare = area
            .height
            .saturating_sub(self.child.desired_height(area.width));
        if spare < 2 || area.width == 0 {
            0
        } else {
            spare.min(self.checklist.desired_height())
        }
    }

    fn child_area(&self, area: Rect) -> Rect {
        let panel = self.panel_height(area);
        Rect::new(
            area.x,
            area.y.saturating_add(panel),
            area.width,
            area.height.saturating_sub(panel),
        )
    }
}

impl Renderable for ChecklistComposition<'_> {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        let height = self.panel_height(area);
        if height > 0 {
            Widget::render(
                Paragraph::new(self.checklist.lines(height)),
                Rect::new(area.x, area.y, area.width, height),
                buffer,
            );
        }
        self.child.render(self.child_area(area), buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.child
            .desired_height(width)
            .saturating_add(self.checklist.desired_height())
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.child.cursor_pos(self.child_area(area))
    }

    fn cursor_style(&self, area: Rect) -> SetCursorStyle {
        self.child.cursor_style(self.child_area(area))
    }
}

#[cfg(test)]
#[path = "plan_checklist_tests.rs"]
mod tests;
