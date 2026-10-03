use super::tests::new_test_composer;
use crate::render::renderable::Renderable;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

#[test]
fn shell_keeps_unicode_multiline_cursor_and_terminal_surface() {
    let (mut composer, _rx) = new_test_composer();
    let draft = "équipe 研究\nsecond line";
    composer.set_text_content(draft.to_owned(), Vec::new(), Vec::new());
    let area = Rect::new(
        /*x*/ 2, /*y*/ 3, /*width*/ 40, /*height*/ 7,
    );
    let before = composer.cursor_pos(area);
    let mut buffer = Buffer::empty(area);
    for cell in &mut buffer.content {
        cell.set_bg(Color::Blue);
    }
    composer.render(area, &mut buffer);
    assert_eq!(composer.cursor_pos(area), before);
    assert_eq!(composer.draft.textarea.text(), draft);
    assert_eq!(buffer[(2, 4)].symbol(), "❯");
    assert_eq!(buffer[(2, 4)].bg, Color::Reset);
    let cursor = before.expect("editable multiline draft has a cursor");
    assert!(area.contains(cursor.into()));
    insta::assert_snapshot!("claudex_unicode_multiline_shell", format!("{buffer:?}"));
}

#[test]
fn shell_renders_zero_and_narrow_areas_without_mutating_the_draft() {
    let (mut composer, _rx) = new_test_composer();
    composer.set_text_content("é研究\ntext".to_owned(), Vec::new(), Vec::new());
    let elements = composer.draft.textarea.text_elements();
    let selection = composer.draft.textarea.mouse_selection_range();
    let cursor = composer.draft.textarea.cursor();
    for width in 0..12 {
        for height in 0..6 {
            let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
            let mut buffer = Buffer::empty(area);
            composer.render(area, &mut buffer);
            assert_eq!(composer.draft.textarea.text(), "é研究\ntext");
            assert_eq!(composer.draft.textarea.text_elements(), elements);
            assert_eq!(composer.draft.textarea.mouse_selection_range(), selection);
            assert_eq!(composer.draft.textarea.cursor(), cursor);
            if let Some(position) = composer.cursor_pos(area) {
                assert!(area.contains(position.into()));
            }
        }
    }
}
