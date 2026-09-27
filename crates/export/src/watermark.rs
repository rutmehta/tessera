//! Deterministic document-space watermark compositor. No installed-font lookup:
//! the caller supplies the font file, so exports are reproducible across hosts.
use crate::encode_error;
use engine_api::{EngineResult, jobs::CancellationToken};
use image::{Rgb32FImage, Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}
impl Anchor {
    fn axes(self) -> (u8, u8) {
        match self {
            Self::TopLeft => (0, 0),
            Self::Top => (1, 0),
            Self::TopRight => (2, 0),
            Self::Left => (0, 1),
            Self::Center => (1, 1),
            Self::Right => (2, 1),
            Self::BottomLeft => (0, 2),
            Self::Bottom => (1, 2),
            Self::BottomRight => (2, 2),
        }
    }
}

/// Size, scale and inset are fractions of the final image's short edge.
/// Text RGB and graphic samples are interpreted in the output document space.
/// Graphics are required to be PNG, with straight alpha; resampling uses
/// premultiplied samples to avoid fringes. Text uses an explicit TrueType font.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Watermark {
    Text {
        text: String,
        font: PathBuf,
        size: f32,
        color: [f32; 3],
        opacity: f32,
        anchor: Anchor,
        inset: f32,
        rotation: f32,
    },
    Graphic {
        path: PathBuf,
        scale: f32,
        opacity: f32,
        anchor: Anchor,
        inset: f32,
    },
}
impl Watermark {
    pub fn validate(&self) -> EngineResult<()> {
        let (size, opacity, inset) = match self {
            Self::Text {
                text,
                font,
                size,
                color,
                opacity,
                inset,
                rotation,
                ..
            } => {
                if text.is_empty()
                    || text.len() > 4096
                    || font.as_os_str().is_empty()
                    || !rotation.is_finite()
                    || rotation.abs() > 360.0
                    || color
                        .iter()
                        .any(|c| !c.is_finite() || !(0.0..=1.0).contains(c))
                {
                    return Err(encode_error("invalid text watermark"));
                }
                (*size, *opacity, *inset)
            }
            Self::Graphic {
                path,
                scale,
                opacity,
                inset,
                ..
            } => {
                if path.as_os_str().is_empty() {
                    return Err(encode_error("empty watermark path"));
                }
                (*scale, *opacity, *inset)
            }
        };
        if !size.is_finite()
            || size <= 0.0
            || size > 1.0
            || !opacity.is_finite()
            || !(0.0..=1.0).contains(&opacity)
            || !inset.is_finite()
            || !(0.0..=0.5).contains(&inset)
        {
            return Err(encode_error(
                "watermark size must be (0,1], opacity [0,1], inset [0,0.5]",
            ));
        }
        Ok(())
    }
}

fn bounded_file(path: &Path) -> EngineResult<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(encode_error)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(encode_error)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(encode_error("watermark asset exceeds 16 MiB"));
    }
    Ok(bytes)
}
fn dimensions(w: u32, h: u32) -> EngineResult<()> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
        return Err(encode_error("watermark exceeds 16 megapixels or is empty"));
    }
    Ok(())
}

/// Apply after resizing and sharpening. This function never mutates a source
/// asset and never writes files. Cancellation is checked before every row.
pub fn apply_watermark(
    dst: &mut Rgb32FImage,
    mark: &Watermark,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    cancel.check()?;
    mark.validate()?;
    let short = dst.width().min(dst.height()) as f32;
    if short == 0.0 {
        return Err(encode_error("empty watermark destination"));
    }
    let (stamp, opacity, anchor, inset, rotation) = match mark {
        Watermark::Graphic {
            path,
            scale,
            opacity,
            anchor,
            inset,
        } => {
            let bytes = bounded_file(path)?;
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(bytes),
                image::ImageFormat::Png,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(16384);
            limits.max_image_height = Some(16384);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            let img = reader.decode().map_err(encode_error)?.into_rgba32f();
            dimensions(img.width(), img.height())?;
            let w = (short * scale).round().max(1.0) as u32;
            let h = ((w as f64 * img.height() as f64 / img.width() as f64)
                .round()
                .max(1.0)) as u32;
            dimensions(w, h)?;
            let mut img = img;
            for p in img.pixels_mut() {
                for c in 0..3 {
                    p[c] *= p[3];
                }
            }
            (
                image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle),
                *opacity,
                *anchor,
                *inset,
                0.0,
            )
        }
        Watermark::Text {
            text,
            font,
            size,
            color,
            opacity,
            anchor,
            inset,
            rotation,
        } => {
            use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
            let font =
                fontdue::Font::from_bytes(bounded_file(font)?, fontdue::FontSettings::default())
                    .map_err(encode_error)?;
            let px = (short * size).max(1.0);
            if px > 4096.0 {
                return Err(encode_error("watermark font exceeds 4096 pixels"));
            }
            let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
            layout.reset(&LayoutSettings::default());
            layout.append(&[&font], &TextStyle::new(text, px, 0));
            let glyphs = layout.glyphs();
            let left = glyphs.iter().map(|g| g.x).fold(0.0f32, f32::min).floor();
            let top = glyphs.iter().map(|g| g.y).fold(0.0f32, f32::min).floor();
            let w = glyphs
                .iter()
                .map(|g| g.x + g.width as f32 - left)
                .fold(1.0f32, f32::max)
                .ceil() as u32;
            let h = glyphs
                .iter()
                .map(|g| g.y + g.height as f32 - top)
                .fold(1.0f32, f32::max)
                .ceil() as u32;
            dimensions(w, h)?;
            let mut stamp = Rgba32FImage::new(w, h);
            for glyph in glyphs {
                cancel.check()?;
                let (metrics, alpha) = font.rasterize_config(glyph.key);
                for (i, a) in alpha.iter().enumerate() {
                    let x = (glyph.x - left) as u32 + (i % metrics.width) as u32;
                    let y = (glyph.y - top) as u32 + (i / metrics.width) as u32;
                    if x < w && y < h {
                        let a = *a as f32 / 255.0;
                        let p = stamp.get_pixel_mut(x, y);
                        let a = a + p[3] * (1.0 - a);
                        *p = Rgba([color[0] * a, color[1] * a, color[2] * a, a]);
                    }
                }
            }
            (stamp, *opacity, *anchor, *inset, *rotation)
        }
    };
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (w, h) = (stamp.width() as f32, stamp.height() as f32);
    let rw = (w * cos.abs() + h * sin.abs()).round().max(1.0);
    let rh = (h * cos.abs() + w * sin.abs()).round().max(1.0);
    let (ax, ay) = anchor.axes();
    let position = |extent: f32, size: f32, axis| match axis {
        0 => short * inset,
        1 => (extent - size) / 2.0,
        _ => extent - size - short * inset,
    };
    let left = position(dst.width() as f32, rw, ax).round();
    let top = position(dst.height() as f32, rh, ay).round();
    for y in 0..dst.height() {
        cancel.check()?;
        for x in 0..dst.width() {
            let dx = x as f32 + 0.5 - left - rw / 2.0;
            let dy = y as f32 + 0.5 - top - rh / 2.0;
            let sx = cos * dx + sin * dy + w / 2.0;
            let sy = -sin * dx + cos * dy + h / 2.0;
            if sx >= 0.0 && sy >= 0.0 && sx < w && sy < h {
                let p = stamp.get_pixel(sx as u32, sy as u32);
                let d = dst.get_pixel_mut(x, y);
                for c in 0..3 {
                    d[c] = p[c] * opacity + d[c] * (1.0 - p[3] * opacity);
                }
            }
        }
    }
    Ok(())
}
