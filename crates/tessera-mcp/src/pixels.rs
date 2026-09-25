use engine_api::{EngineError, EngineResult, recipe::DevelopSettings, tools::Histogram};
use pipeline_cpu::{Image, RenderSource};
use std::path::Path;

pub(crate) enum Source {
    Rgb(Image),
    Raw(Box<(raw_decode::CfaImage, raw_decode::RawMetadata)>),
}
pub(crate) fn is_rgb(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "tif" | "tiff"
        )
    })
}
impl Source {
    pub(crate) fn open(path: &Path) -> EngineResult<Self> {
        if !is_rgb(path) {
            let mut raw = raw_decode::RawSource::open(path)?;
            let cfa = raw.decode_cfa()?;
            return Ok(Self::Raw(Box::new((cfa, raw.metadata()))));
        }
        let rgb = image::open(path)
            .map_err(|e| EngineError::Decode {
                format: "image".into(),
                message: e.to_string(),
            })?
            .into_rgb32f();
        let mut planes = vec![vec![0.; rgb.width() as usize * rgb.height() as usize]; 3];
        for (i, pixel) in rgb.pixels().enumerate() {
            let linear = pixel.0.map(|v| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            });
            for (c, row) in [
                [0.6274, 0.3293, 0.0433],
                [0.0691, 0.9195, 0.0114],
                [0.0164, 0.0880, 0.8956],
            ]
            .iter()
            .enumerate()
            {
                planes[c][i] = row.iter().zip(linear).map(|(a, b)| a * b).sum();
            }
        }
        Ok(Self::Rgb(Image::new(rgb.width(), rgb.height(), planes)?))
    }
    pub(crate) fn borrowed(&self) -> RenderSource<'_> {
        match self {
            Self::Rgb(image) => RenderSource::Rgb(image),
            Self::Raw(raw) => RenderSource::Cfa {
                image: &raw.0,
                metadata: &raw.1,
            },
        }
    }
    pub(crate) fn orientation(&self) -> u16 {
        match self {
            Self::Raw(raw) => raw.1.orientation,
            _ => 1,
        }
    }
}
pub(crate) fn render(
    path: &Path,
    settings: &DevelopSettings,
    max: u32,
) -> EngineResult<image::RgbImage> {
    if !(1..=4096).contains(&max) {
        return Err(EngineError::invalid("max_px", "must be 1..=4096"));
    }
    let source = Source::open(path)?;
    let rgb = pipeline_cpu::render(settings, &source.borrowed())?;
    let rgb = orient(rgb, source.orientation());
    if rgb.width().max(rgb.height()) <= max {
        return Ok(rgb);
    }
    Ok(image::DynamicImage::ImageRgb8(rgb)
        .resize(max, max, image::imageops::FilterType::Triangle)
        .to_rgb8())
}
fn orient(rgb: image::RgbImage, orientation: u16) -> image::RgbImage {
    use image::imageops::*;
    match orientation {
        2 => flip_horizontal(&rgb),
        3 => rotate180(&rgb),
        4 => flip_vertical(&rgb),
        5 => rotate90(&flip_vertical(&rgb)),
        6 => rotate90(&rgb),
        7 => rotate90(&flip_horizontal(&rgb)),
        8 => rotate270(&rgb),
        _ => rgb,
    }
}
pub(crate) fn histogram(rgb: &image::RgbImage, bins: u16) -> EngineResult<Histogram> {
    if !(2..=4096).contains(&bins) {
        return Err(EngineError::invalid("bins", "must be 2..=4096"));
    }
    let mut out = Histogram {
        red: vec![0; bins as usize],
        green: vec![0; bins as usize],
        blue: vec![0; bins as usize],
        luminance: vec![0; bins as usize],
        ..Default::default()
    };
    let bin = |v: f32| ((v.clamp(0., 1.) * f32::from(bins)) as usize).min(bins as usize - 1);
    for p in rgb.pixels() {
        out.red[bin(f32::from(p[0]) / 255.)] += 1;
        out.green[bin(f32::from(p[1]) / 255.)] += 1;
        out.blue[bin(f32::from(p[2]) / 255.)] += 1;
        out.luminance[bin(luma(p))] += 1;
        out.clipped_shadows += f32::from(p.0.contains(&0));
        out.clipped_highlights += f32::from(p.0.contains(&255));
    }
    let count = (rgb.width() as f32 * rgb.height() as f32).max(1.);
    out.clipped_shadows /= count;
    out.clipped_highlights /= count;
    Ok(out)
}
pub(crate) fn luma(p: &image::Rgb<u8>) -> f32 {
    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.
}

pub(crate) fn linear_histogram(
    path: &Path,
    settings: &DevelopSettings,
    bins: u16,
) -> EngineResult<Histogram> {
    if !(2..=4096).contains(&bins) {
        return Err(EngineError::invalid("bins", "must be 2..=4096"));
    }
    let source = Source::open(path)?;
    let linear = pipeline_cpu::render_linear_scaled(settings, &source.borrowed(), 1)?;
    let mut out = Histogram {
        red: vec![0; bins as usize],
        green: vec![0; bins as usize],
        blue: vec![0; bins as usize],
        luminance: vec![0; bins as usize],
        ..Default::default()
    };
    let bin = |v: f32| ((v.clamp(0., 1.) * f32::from(bins)) as usize).min(bins as usize - 1);
    let planes = linear.planes();
    for ((red, green), blue) in planes[0].iter().zip(&planes[1]).zip(&planes[2]) {
        let p = [*red, *green, *blue];
        out.red[bin(p[0])] += 1;
        out.green[bin(p[1])] += 1;
        out.blue[bin(p[2])] += 1;
        out.luminance[bin(0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2])] += 1;
        out.clipped_shadows += f32::from(p.iter().any(|v| *v <= 0.));
        out.clipped_highlights += f32::from(p.iter().any(|v| *v >= 1.));
    }
    let count = planes[0].len().max(1) as f32;
    out.clipped_shadows /= count;
    out.clipped_highlights /= count;
    Ok(out)
}
