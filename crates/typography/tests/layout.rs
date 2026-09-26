use typography::*;
#[test]
fn baseline_shift_and_tracking_apply_without_moving_the_baseline() {
    let r = renderer();
    let mut text = TextModel::point("abc", "Noto Sans", 20.0);
    let plain = r.layout(&text).unwrap();
    text.runs[0].baseline_shift = 4.0;
    text.runs[0].tracking = 2.0;
    let shifted = r.layout(&text).unwrap();
    assert_eq!(shifted.lines[0].baseline, plain.lines[0].baseline);
    assert_eq!(shifted.glyphs[0].y, plain.glyphs[0].y - 4.0);
    assert!((shifted.lines[0].width - plain.lines[0].width - 6.0).abs() < 0.001);
}
fn renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    r.fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Regular.ttf").to_vec());
    r
}
#[test]
fn paragraph_alignment_and_justification() {
    let r = renderer();
    let mut text = TextModel::point("one two three four five six", "Noto Sans", 20.0);
    text.text_box = TextBox::Paragraph {
        width: 150.0,
        height: 400.0,
    };
    let left = r.layout(&text).unwrap();
    assert!(left.lines.len() > 1);
    text.paragraph.alignment = Alignment::Center;
    let centered = r.layout(&text).unwrap();
    assert!((centered.lines[0].x - (150.0 - left.lines[0].width) / 2.0).abs() < 0.001);
    text.paragraph.alignment = Alignment::Right;
    let right = r.layout(&text).unwrap();
    assert!((right.lines[0].x + right.lines[0].width - 150.0).abs() < 0.001);
    text.paragraph.alignment = Alignment::Justify;
    let justified = r.layout(&text).unwrap();
    assert!((justified.lines[0].width - 150.0).abs() < 0.001);
    assert!(justified.lines.last().unwrap().width < 150.0);
}
#[test]
fn explicit_leading_indents_and_paragraph_spacing() {
    let r = renderer();
    let mut text = TextModel::point("one\ntwo", "Noto Sans", 20.0);
    text.runs[0].leading = 15.0;
    text.paragraph.left_indent = 10.0;
    text.paragraph.first_line_indent = 5.0;
    text.paragraph.space_before = 2.0;
    text.paragraph.space_after = 3.0;
    let layout = r.layout(&text).unwrap();
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].x, 15.0);
    assert!((layout.lines[1].baseline - layout.lines[0].baseline - 20.0).abs() < 0.001);
}
#[test]
fn mandatory_unicode_line_separator_is_not_a_space() {
    let r = renderer();
    let text = TextModel::point("one\u{2028}two", "Noto Sans", 20.0);
    assert_eq!(r.layout(&text).unwrap().lines.len(), 2);
}
#[test]
fn mixed_bidi_clusters_are_visual_but_keep_source_offsets() {
    let r = renderer();
    let text = TextModel::point("a אבג z", "Noto Sans", 20.0);
    let glyphs = r.layout(&text).unwrap().glyphs;
    let rtl: Vec<_> = glyphs
        .iter()
        .filter(|g| (2..8).contains(&g.cluster))
        .map(|g| g.cluster)
        .collect();
    assert_eq!(rtl, vec![6, 4, 2]);
    assert!(glyphs.windows(2).all(|g| g[1].x >= g[0].x));
}
#[test]
fn paragraph_overflow_empty_text_and_missing_fonts_are_explicit() {
    let r = renderer();
    let mut text = TextModel::point("longword", "Noto Sans", 20.0);
    text.text_box = TextBox::Paragraph {
        width: 1.0,
        height: 100.0,
    };
    assert!(r.layout(&text).unwrap().overflow);
    text.text_box = TextBox::Paragraph {
        width: 100.0,
        height: 1.0,
    };
    let layout = r.layout(&text).unwrap();
    assert!(layout.overflow && layout.glyphs.is_empty());
    assert!(r.layout(&TextModel::default()).unwrap().glyphs.is_empty());
    text.vertical = true;
    assert!(matches!(r.layout(&text), Err(Error::UnsupportedVertical)));
    text.vertical = false;
    text.runs[0].family = "not-a-real-font".into();
    assert!(matches!(r.layout(&text), Err(Error::MissingFont(_))));
}
