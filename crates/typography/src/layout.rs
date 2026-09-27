use crate::{Alignment, Error, Result, TextBox, TextModel, TextRun};
use fontdb::{Database, Family, ID, Query, Style, Weight};
use rustybuzz::{Direction, Feature, UnicodeBuffer, Variation};
use std::{ops::Range, path::Path};
use unicode_bidi::BidiInfo;

/// A glyph origin and advance in document pixels; clusters index UTF-8 bytes
/// in the concatenated source. Font IDs are local to this renderer, not wire IDs.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub id: u16,
    pub font: ID,
    pub run: usize,
    pub cluster: usize,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    /// Clockwise rotation in screen coordinates, radians.
    pub angle: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub glyphs: Range<usize>,
    pub source: Range<usize>,
    pub x: f32,
    pub baseline: f32,
    pub width: f32,
    pub available_width: f32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub glyphs: Vec<Glyph>,
    pub lines: Vec<Line>,
    pub overflow: bool,
}
/// Owns font discovery and resolution. `new` is deliberately hermetic; call
/// `discover_system_fonts` explicitly for macOS / host fonts.
#[derive(Default)]
pub struct TextRenderer {
    pub(crate) fonts: Database,
    pub(crate) shaped: crate::cache::Cache<Vec<Glyph>>,
    paragraphs: crate::cache::Cache<(Layout, f32)>,
    pub(crate) contours: crate::cache::Cache<lyon_path::Path>,
}
impl TextRenderer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fonts(&self) -> &Database {
        &self.fonts
    }
    pub fn fonts_mut(&mut self) -> &mut Database {
        self.shaped.clear();
        self.paragraphs.clear();
        self.contours.clear();
        &mut self.fonts
    }
    pub fn discover_system_fonts(&mut self) {
        self.fonts_mut().load_system_fonts();
    }
    pub fn load_font_dir(&mut self, path: impl AsRef<Path>) {
        self.fonts_mut().load_fonts_dir(path);
    }
    fn resolve(&self, run: &TextRun) -> Result<ID> {
        let family = match run.family.as_str() {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            name => Family::Name(name),
        };
        self.fonts
            .query(&Query {
                families: &[family],
                weight: Weight(run.weight),
                style: if run.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Query::default()
            })
            .or_else(|| {
                self.fonts
                    .faces()
                    .find(|f| f.post_script_name == run.family)
                    .map(|f| f.id)
            })
            .ok_or_else(|| Error::MissingFont(run.family.clone()))
    }
    fn shape_line(
        &self,
        model: &TextModel,
        text: &str,
        spans: &[(Range<usize>, ID)],
        bidi: &BidiInfo<'_>,
        para: &unicode_bidi::ParagraphInfo,
        range: Range<usize>,
    ) -> Result<Vec<Glyph>> {
        let mut glyphs = Vec::new();
        if range.is_empty() {
            return Ok(glyphs);
        }
        let source_start = range.start;
        let (levels, visual) = bidi.visual_runs(para, range.clone());
        let mut run_map = Vec::new();
        let pieces: Vec<_> = spans
            .iter()
            .enumerate()
            .filter_map(|(i, (span, font))| {
                let start = span.start.max(range.start);
                let end = span.end.min(range.end);
                if start >= end {
                    return None;
                }
                let mut run = model.runs[i].clone();
                run.text = text[start..end].to_owned();
                run_map.push(i);
                Some((
                    format!("{font:?}"),
                    start - range.start,
                    end - range.start,
                    run,
                ))
            })
            .collect();
        let directions: Vec<_> = visual
            .iter()
            .map(|r| {
                (
                    r.start - range.start,
                    r.end - range.start,
                    levels[r.start].is_rtl(),
                )
            })
            .collect();
        let cache_key = serde_json::to_vec(&(pieces, directions))?;
        if let Some(mut cached) = self.shaped.get(&cache_key) {
            for g in &mut cached {
                g.cluster += source_start;
                g.run = run_map[g.run];
            }
            return Ok(cached);
        }
        let mut x = 0.0;
        for direction_run in visual {
            let rtl = levels[direction_run.start].is_rtl();
            let mut pieces: Vec<_> = spans
                .iter()
                .enumerate()
                .filter_map(|(run, (span, font))| {
                    let start = span.start.max(direction_run.start);
                    let end = span.end.min(direction_run.end);
                    (start < end).then_some((run, *font, start..end))
                })
                .collect();
            if rtl {
                pieces.reverse();
            }
            for (run_index, font, span) in pieces {
                let run = &model.runs[run_index];
                let mut shaped = self
                    .fonts
                    .with_face_data(font, |data, index| -> Result<Vec<Glyph>> {
                        let mut face =
                            rustybuzz::Face::from_slice(data, index).ok_or(Error::Font)?;
                        face.set_variations(
                            &run.axes
                                .iter()
                                .map(|(tag, value)| Variation {
                                    tag: ttf_parser::Tag::from_bytes_lossy(tag.as_bytes()),
                                    value: *value,
                                })
                                .collect::<Vec<_>>(),
                        );
                        let scale = run.size / face.units_per_em() as f32;
                        let mut buffer = UnicodeBuffer::new();
                        buffer.push_str(&text[span.clone()]);
                        buffer.set_direction(if rtl {
                            Direction::RightToLeft
                        } else {
                            Direction::LeftToRight
                        });
                        buffer.guess_segment_properties();
                        let mut features: Vec<_> = run
                            .features
                            .iter()
                            .map(|(tag, value)| {
                                Feature::new(
                                    ttf_parser::Tag::from_bytes_lossy(tag.as_bytes()),
                                    *value,
                                    ..,
                                )
                            })
                            .collect();
                        features.push(Feature::new(
                            ttf_parser::Tag::from_bytes(b"kern"),
                            u32::from(run.kerning),
                            ..,
                        ));
                        let result = rustybuzz::shape(&face, &features, buffer);
                        let mut out = Vec::new();
                        for (i, (info, pos)) in result
                            .glyph_infos()
                            .iter()
                            .zip(result.glyph_positions())
                            .enumerate()
                        {
                            let cluster_end = result
                                .glyph_infos()
                                .get(i + 1)
                                .is_none_or(|next| next.cluster != info.cluster);
                            let advance = pos.x_advance as f32 * scale
                                + if cluster_end { run.tracking } else { 0.0 };
                            out.push(Glyph {
                                id: info.glyph_id as u16,
                                font,
                                run: run_index,
                                cluster: span.start + info.cluster as usize,
                                x: x + pos.x_offset as f32 * scale,
                                y: -pos.y_offset as f32 * scale - run.baseline_shift,
                                advance,
                                angle: 0.0,
                            });
                            x += advance;
                        }
                        Ok(out)
                    })
                    .ok_or(Error::Font)??;
                glyphs.append(&mut shaped);
            }
        }
        let mut cached = glyphs.clone();
        for g in &mut cached {
            g.cluster -= source_start;
            g.run = run_map.binary_search(&g.run).unwrap();
        }
        self.shaped.insert(
            cache_key,
            cached,
            glyphs.len() * std::mem::size_of::<Glyph>(),
        );
        Ok(glyphs)
    }
    pub fn layout(&self, model: &TextModel) -> Result<Layout> {
        model.validate()?;
        if model.vertical {
            return Err(Error::UnsupportedVertical);
        }
        let text: String = model.runs.iter().map(|r| r.text.as_str()).collect();
        if text.is_empty() {
            return Ok(Layout::default());
        }
        let mut offset = 0;
        let mut spans = Vec::new();
        for run in &model.runs {
            let end = offset + run.text.len();
            spans.push((offset..end, self.resolve(run)?));
            offset = end;
        }
        let bidi = BidiInfo::new(&text, None);
        let p = &model.paragraph;
        let mut layout = Layout::default();
        let mut y = 0.0_f32;
        for para in &bidi.paragraphs {
            // Paragraph-local source offsets make unchanged paragraphs reusable
            // after edits before them. Include incoming y for bit-exact f32
            // placement; when it changes, shaped line entries still survive.
            let mut run_map = Vec::new();
            let runs: Vec<_> = spans
                .iter()
                .enumerate()
                .filter_map(|(i, (span, font))| {
                    let start = span.start.max(para.range.start);
                    let end = span.end.min(para.range.end);
                    if start >= end {
                        return None;
                    }
                    let mut run = model.runs[i].clone();
                    run.text = text[start..end].to_owned();
                    run_map.push(i);
                    Some((format!("{font:?}"), start - para.range.start, run))
                })
                .collect();
            let paragraph_key = serde_json::to_vec(&(
                runs,
                &model.paragraph,
                model.text_box,
                y.to_bits(),
                model.runs[0].size.to_bits(),
            ))?;
            if let Some((mut cached, next_y)) = self.paragraphs.get(&paragraph_key) {
                let glyph_start = layout.glyphs.len();
                for g in &mut cached.glyphs {
                    g.cluster += para.range.start;
                    g.run = run_map[g.run];
                }
                for l in &mut cached.lines {
                    l.source.start += para.range.start;
                    l.source.end += para.range.start;
                    l.glyphs.start += glyph_start;
                    l.glyphs.end += glyph_start;
                }
                layout.glyphs.extend(cached.glyphs);
                layout.lines.extend(cached.lines);
                layout.overflow |= cached.overflow;
                y = next_y;
                continue;
            }
            let glyph_start = layout.glyphs.len();
            let line_start = layout.lines.len();
            let previous_overflow = layout.overflow;
            layout.overflow = false;

            y += p.space_before;
            let end = text[para.range.clone()]
                .trim_end_matches(['\n', '\r', '\u{2029}', '\u{2028}'])
                .len()
                + para.range.start;
            let mut start = para.range.start;
            let breaks: Vec<_> = unicode_linebreak::linebreaks(&text[start..end])
                .map(|(i, kind)| {
                    (
                        start + i,
                        kind == unicode_linebreak::BreakOpportunity::Mandatory,
                    )
                })
                .filter(|(_, mandatory)| *mandatory || !matches!(model.text_box, TextBox::Point))
                .collect();
            let mut first = true;
            loop {
                let left = p.left_indent + if first { p.first_line_indent } else { 0.0 };
                let available = match model.text_box {
                    TextBox::Point => f32::INFINITY,
                    TextBox::Paragraph { width, .. } => width - p.right_indent - left,
                };
                let mut chosen = end;
                let mut forced = false;
                let mut glyphs = Vec::new();
                for (candidate, mandatory) in breaks.iter().copied().filter(|(b, _)| *b > start) {
                    let visible = text[start..candidate].trim_end_matches([
                        '\n', '\r', '\u{2028}', '\u{2029}', '\u{b}', '\u{c}', '\u{85}',
                    ]);
                    let visible = if matches!(model.text_box, TextBox::Paragraph { .. }) {
                        visible.trim_end_matches([' ', '\t'])
                    } else {
                        visible
                    };
                    let visible_end = start + visible.len();
                    let trial =
                        self.shape_line(model, &text, &spans, &bidi, para, start..visible_end)?;
                    let width: f32 = trial.iter().map(|g| g.advance).sum();
                    if width > available && !glyphs.is_empty() {
                        break;
                    }
                    glyphs = trial;
                    chosen = candidate;
                    forced = mandatory;
                    if width > available || mandatory {
                        break;
                    } // unbreakable token: preserve cluster, report overflow
                }
                let mut width: f32 = glyphs.iter().map(|g| g.advance).sum();
                let mut ascent: f32 = 0.0;
                let mut descent: f32 = 0.0;
                let mut leading: f32 = 0.0;
                for (i, (span, font)) in spans.iter().enumerate() {
                    if span.start >= chosen || span.end <= start {
                        continue;
                    }
                    let run = &model.runs[i];
                    let (a, d) = self
                        .fonts
                        .with_face_data(*font, |data, index| {
                            let face =
                                ttf_parser::Face::parse(data, index).map_err(|_| Error::Font)?;
                            Ok::<_, Error>((
                                face.ascender() as f32 * run.size / face.units_per_em() as f32,
                                -face.descender() as f32 * run.size / face.units_per_em() as f32,
                            ))
                        })
                        .ok_or(Error::Font)??;
                    ascent = ascent.max(a);
                    descent = descent.max(d - run.baseline_shift);
                    leading = leading.max(if run.leading == 0.0 {
                        run.size * 1.2
                    } else {
                        run.leading
                    });
                }
                if leading == 0.0 {
                    leading = model.runs[0].size * 1.2;
                    ascent = model.runs[0].size;
                }
                let baseline = y + ascent;
                let spaces = glyphs
                    .iter()
                    .filter(|g| text[g.cluster..].starts_with(' '))
                    .count();
                let extra = if p.alignment == Alignment::Justify
                    && chosen < end
                    && !forced
                    && spaces > 0
                    && available.is_finite()
                {
                    (available - width).max(0.0) / spaces as f32
                } else {
                    0.0
                };
                let shift = if available.is_finite() {
                    match p.alignment {
                        Alignment::Center => (available - width) / 2.0,
                        Alignment::Right => available - width,
                        _ => 0.0,
                    }
                } else {
                    match p.alignment {
                        Alignment::Center => -width / 2.0,
                        Alignment::Right => -width,
                        _ => 0.0,
                    }
                };
                let mut cumulative = 0.0;
                for g in &mut glyphs {
                    g.x += left + shift + cumulative;
                    g.y += baseline;
                    if text[g.cluster..].starts_with(' ') {
                        g.advance += extra;
                        cumulative += extra;
                    }
                }
                width += cumulative;
                let index = layout.glyphs.len();
                let visible = match model.text_box {
                    TextBox::Point => true,
                    TextBox::Paragraph { height, .. } => baseline + descent <= height,
                };
                layout.overflow |= !visible || width > available;
                if visible {
                    layout.glyphs.extend(glyphs);
                    layout.lines.push(Line {
                        glyphs: index..layout.glyphs.len(),
                        source: start..chosen,
                        x: left + shift,
                        baseline,
                        width,
                        available_width: available,
                    });
                }
                y += leading;
                if chosen >= end {
                    break;
                }
                start = chosen;
                first = false;
            }
            y += p.space_after;
            let mut cached = Layout {
                glyphs: layout.glyphs[glyph_start..].to_vec(),
                lines: layout.lines[line_start..].to_vec(),
                overflow: layout.overflow,
            };
            for g in &mut cached.glyphs {
                g.cluster -= para.range.start;
                g.run = run_map.binary_search(&g.run).unwrap();
            }
            for l in &mut cached.lines {
                l.source.start -= para.range.start;
                l.source.end -= para.range.start;
                l.glyphs.start -= glyph_start;
                l.glyphs.end -= glyph_start;
            }
            let bytes = cached.glyphs.len() * std::mem::size_of::<Glyph>()
                + cached.lines.len() * std::mem::size_of::<Line>();
            self.paragraphs.insert(paragraph_key, (cached, y), bytes);
            layout.overflow |= previous_overflow;
        }
        Ok(layout)
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    fn renderer() -> TextRenderer {
        let mut r = TextRenderer::new();
        r.fonts_mut()
            .load_font_data(include_bytes!("../tests/fonts/NotoSans-Regular.ttf").to_vec());
        r
    }
    #[test]
    fn paragraph_cache_rebases_sources_and_preserves_reflow() {
        let r = renderer();
        let mut model = TextModel::point(
            "One paragraph.\nTwo paragraphs.\nThird ffi e\u{301}.",
            "Noto Sans",
            21.,
        );
        model.text_box = TextBox::Paragraph {
            width: 170.,
            height: 500.,
        };
        for text in [
            "One paragraph.\nTwo paragraphs.\nThird ffi e\u{301}.",
            "Other!\nTwo paragraphs.\nThird ffi e\u{301}.",
            "Much much longer first paragraph to reflow.\nTwo paragraphs.\nThird ffi e\u{301}.",
            "\nTwo paragraphs.\nThird ffi e\u{301}.",
        ] {
            model.runs[0].text = text.into();
            assert_eq!(
                r.layout(&model).unwrap(),
                renderer().layout(&model).unwrap()
            );
        }
        // Mutating font discovery invalidates entries carrying database-local IDs.
        let mut r = r;
        let before = r.layout(&model).unwrap();
        r.fonts_mut();
        assert_eq!(before, r.layout(&model).unwrap());
    }
    #[test]
    fn blank_paragraph_cache_tracks_empty_first_run_fallback() {
        let r = renderer();
        let mut model = TextModel::point("", "Noto Sans", 12.);
        model.runs.push(crate::TextRun {
            text: "\nX".into(),
            family: "Noto Sans".into(),
            size: 20.,
            ..Default::default()
        });
        r.layout(&model).unwrap();
        model.runs[0].size = 40.;
        assert_eq!(
            r.layout(&model).unwrap(),
            renderer().layout(&model).unwrap()
        );
    }
    #[test]
    fn inserting_a_run_only_reshapes_the_changed_paragraph() {
        let r = renderer();
        let mut model = TextModel::point("First.\n", "Noto Sans", 21.);
        let mut second = model.runs[0].clone();
        second.text = "Second ffi.".into();
        model.runs.push(second);
        r.layout(&model).unwrap();
        let shaped = r.shaped.entry_count();
        let paragraphs = r.paragraphs.entry_count();
        let mut inserted = model.runs[0].clone();
        inserted.text = "New ".into();
        model.runs.insert(0, inserted);
        assert_eq!(
            r.layout(&model).unwrap(),
            renderer().layout(&model).unwrap()
        );
        assert_eq!(r.shaped.entry_count(), shaped + 1);
        assert_eq!(r.paragraphs.entry_count(), paragraphs + 1);
    }
}
