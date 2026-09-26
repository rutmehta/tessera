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
}
impl TextRenderer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fonts(&self) -> &Database {
        &self.fonts
    }
    pub fn fonts_mut(&mut self) -> &mut Database {
        &mut self.fonts
    }
    pub fn discover_system_fonts(&mut self) {
        self.fonts.load_system_fonts();
    }
    pub fn load_font_dir(&mut self, path: impl AsRef<Path>) {
        self.fonts.load_fonts_dir(path);
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
        let (levels, visual) = bidi.visual_runs(para, range);
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
        let mut y = 0.0;
        for para in &bidi.paragraphs {
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
        }
        Ok(layout)
    }
}
