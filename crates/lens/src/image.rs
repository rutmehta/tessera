use crate::{Error, Point, Result};
#[derive(Clone, Debug)]
pub struct GrayImage {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) data: Vec<f64>,
}
impl GrayImage {
    pub fn new(width: usize, height: usize, data: Vec<f64>) -> Result<Self> {
        if width < 3
            || height < 3
            || width.checked_mul(height) != Some(data.len())
            || !data.iter().all(|x| x.is_finite())
        {
            return Err(Error::Invalid("invalid image dimensions or pixels".into()));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn pixels(&self) -> &[f64] {
        &self.data
    }
    pub(crate) fn point(&self, x: usize, y: usize) -> Point {
        [
            2. * x as f64 / (self.width - 1) as f64 - 1.,
            2. * y as f64 / (self.height - 1) as f64 - 1.,
        ]
    }
    pub(crate) fn gradient(&self, x: usize, y: usize) -> Point {
        [
            (self.data[y * self.width + x + 1] - self.data[y * self.width + x - 1]) * 0.5,
            (self.data[(y + 1) * self.width + x] - self.data[(y - 1) * self.width + x]) * 0.5,
        ]
    }
}
#[derive(Clone, Debug)]
pub struct RgbImage {
    pub(crate) channels: [GrayImage; 3],
}
impl RgbImage {
    pub fn new(width: usize, height: usize, data: Vec<[f64; 3]>) -> Result<Self> {
        Ok(Self {
            channels: [
                GrayImage::new(width, height, data.iter().map(|p| p[0]).collect())?,
                GrayImage::new(width, height, data.iter().map(|p| p[1]).collect())?,
                GrayImage::new(width, height, data.iter().map(|p| p[2]).collect())?,
            ],
        })
    }
}
#[derive(Clone, Debug)]
pub struct LineSegment {
    pub start: Point,
    pub end: Point,
    pub points: Vec<Point>,
    pub strength: f64,
}
/// LSD-style gradient-orientation region growing; no a-contrario false-alarm model.
/// Threshold is an absolute linear-light central-difference magnitude.
pub fn detect_lines(image: &GrayImage, threshold: f64, min_pixels: usize) -> Vec<LineSegment> {
    if !threshold.is_finite() || threshold <= 0. {
        return Vec::new();
    }
    let w = image.width;
    let h = image.height;
    let mut gradients = vec![[0.; 2]; w * h];
    let mut seeds = Vec::new();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let g = image.gradient(x, y);
            gradients[y * w + x] = g;
            if g[0].hypot(g[1]) >= threshold {
                seeds.push(y * w + x)
            }
        }
    }
    seeds.sort_by(|a, b| {
        let a = gradients[*a];
        let b = gradients[*b];
        b[0].hypot(b[1]).total_cmp(&a[0].hypot(a[1]))
    });
    let mut used = vec![false; w * h];
    let mut lines = Vec::new();
    for seed in seeds {
        if used[seed] {
            continue;
        }
        let g = gradients[seed];
        let norm = g[0].hypot(g[1]);
        let mut queue = vec![seed];
        used[seed] = true;
        let mut points = Vec::new();
        let mut strength = 0.;
        while let Some(i) = queue.pop() {
            let x = i % w;
            let y = i / w;
            points.push(image.point(x, y));
            let a = gradients[i];
            strength += a[0].hypot(a[1]);
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let xx = x as isize + dx;
                    let yy = y as isize + dy;
                    if xx < 1 || yy < 1 || xx >= w as isize - 1 || yy >= h as isize - 1 {
                        continue;
                    }
                    let j = yy as usize * w + xx as usize;
                    if used[j] {
                        continue;
                    }
                    let a = gradients[j];
                    let n = a[0].hypot(a[1]);
                    if n >= threshold && (a[0] * g[0] + a[1] * g[1]).abs() / (n * norm) > 0.85 {
                        used[j] = true;
                        queue.push(j)
                    }
                }
            }
        }
        if points.len() < min_pixels.max(5) {
            continue;
        }
        let (c, d, error, span) = crate::calibration::fit_line(&points);
        if span < 1e-4 || error / span > 0.03 {
            continue;
        }
        points.sort_by(|a, b| {
            ((a[0] - c[0]) * d[0] + (a[1] - c[1]) * d[1])
                .total_cmp(&((b[0] - c[0]) * d[0] + (b[1] - c[1]) * d[1]))
        });
        let start = points[0];
        let end = *points.last().unwrap();
        strength /= points.len() as f64;
        lines.push(LineSegment {
            start,
            end,
            points,
            strength,
        });
    }
    lines
}
