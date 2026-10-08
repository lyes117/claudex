use super::*;
use crate::line_truncation::line_width;
use pretty_assertions::assert_eq;

#[test]
fn banner_fits_every_width_and_preserves_unicode_session_metadata() {
    for width in 0..110 {
        let lines = render(
            BannerContent {
                version: "test",
                greeting: "Welcome to Claudex",
                model: "gpt-6.1-sol · high",
                directory: "C:\\研究\\équipe\\ｶﾞﾊﾟ-project",
                unrestricted: true,
            },
            width,
        );
        assert!(
            lines
                .iter()
                .all(|line| line_width(line) <= usize::from(width))
        );
        if width == 0 {
            assert_eq!(lines, Vec::<Line<'static>>::new());
        }
        if width >= 90 {
            let text = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("gpt-6.1-sol · high"));
            assert!(text.contains("C:\\研究\\équipe\\ｶﾞﾊﾟ-project"));
            assert!(text.contains("permissions: YOLO mode"));
            assert!(text.contains("/agents"));
        }
    }
}

#[test]
fn height_budget_preserves_metadata_before_optional_decoration() {
    let content = BannerContent {
        version: "test",
        greeting: "Welcome to Claudex",
        model: "gpt-6.1-sol",
        directory: "équipe-研究",
        unrestricted: true,
    };
    for width in 0..110 {
        for rows in 0..16 {
            let lines = render_for_height(content, width, rows);
            assert!(
                lines
                    .iter()
                    .all(|line| line_width(line) <= usize::from(width))
            );
            assert!(lines.len() <= usize::from(rows));
            if rows == 0 || width == 0 {
                assert_eq!(lines, Vec::<Line<'static>>::new());
            }
        }
    }
    let lines = render_for_height(content, /*width*/ 48, /*available_rows*/ 5);
    let text = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Claudex vtest"));
    assert!(text.contains("gpt-6.1-sol"));
    assert!(text.contains("équipe-研究"));
    assert!(text.contains("/help"));
    assert!(text.contains("YOLO mode"));
}

#[test]
fn compact_banner_does_not_offer_unrestricted_permissions() {
    let lines = render(
        BannerContent {
            version: "test",
            greeting: "Welcome to Claudex",
            model: "gpt-6.1-sol",
            directory: "project",
            unrestricted: false,
        },
        /*width*/ 44,
    );
    let text = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("YOLO"));
    assert!(text.contains("gpt-6.1-sol"));
    assert!(text.contains("/help"));
    insta::assert_snapshot!("claudex_compact_welcome_shell", text);
}
