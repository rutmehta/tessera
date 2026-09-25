use crate::Face;
use anyhow::{Result, ensure};
use image::{Rgb, RgbImage, imageops};
use ml_runtime::Tensor;

pub const DETECTOR_SIZE: u32 = 640;
pub const CANONICAL: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

#[derive(Debug, Clone, Copy)]
pub struct Letterbox {
    scale: [f32; 2],
    offset: [f32; 2],
}
impl Letterbox {
    pub fn unproject(&self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.offset[0]) / self.scale[0],
            (p[1] - self.offset[1]) / self.scale[1],
        ]
    }
}

/// Centered black padding, BGR values in [0,255], no normalization.
pub fn letterbox(image: &RgbImage) -> Result<(Tensor, Letterbox)> {
    ensure!(image.width() > 0 && image.height() > 0, "empty image");
    let scale = DETECTOR_SIZE as f64 / f64::from(image.width().max(image.height()));
    let w = (f64::from(image.width()) * scale).round().max(1.) as u32;
    let h = (f64::from(image.height()) * scale).round().max(1.) as u32;
    let small = imageops::resize(image, w, h, imageops::FilterType::Triangle);
    let (x, y) = ((DETECTOR_SIZE - w) / 2, (DETECTOR_SIZE - h) / 2);
    let mut padded = RgbImage::new(DETECTOR_SIZE, DETECTOR_SIZE);
    imageops::replace(&mut padded, &small, i64::from(x), i64::from(y));
    Ok((
        planes(&padded, true)?,
        Letterbox {
            scale: [
                w as f32 / image.width() as f32,
                h as f32 / image.height() as f32,
            ],
            offset: [x as f32, y as f32],
        },
    ))
}

pub(crate) fn planes(image: &RgbImage, bgr: bool) -> Result<Tensor> {
    let mut data = Vec::with_capacity(image.as_raw().len());
    for c in 0..3 {
        data.extend(
            image
                .pixels()
                .map(|p| f32::from(p[if bgr { 2 - c } else { c }])),
        );
    }
    Tensor::new(3, image.height() as usize, image.width() as usize, data)
}

pub(crate) fn validate(face: &Face) -> Result<()> {
    ensure!(
        face.bbox
            .iter()
            .chain(face.landmarks5.iter().flatten())
            .all(|v| v.is_finite()),
        "nonfinite face geometry"
    );
    ensure!(
        face.bbox[2] > 0.
            && face.bbox[3] > 0.
            && (face.bbox[0] + face.bbox[2]).is_finite()
            && (face.bbox[1] + face.bbox[3]).is_finite(),
        "invalid face extent"
    );
    ensure!((0.0..=1.0).contains(&face.score), "invalid confidence");
    Ok(())
}

pub fn nms(mut faces: Vec<Face>, threshold: f32) -> Result<Vec<Face>> {
    ensure!((0.0..=1.0).contains(&threshold), "invalid NMS threshold");
    for f in &faces {
        validate(f)?;
    }
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<Face> = Vec::new();
    for face in faces {
        if keep.iter().all(|f| iou(f.bbox, face.bbox) <= threshold) {
            keep.push(face);
        }
    }
    Ok(keep)
}
fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let a = a.map(f64::from);
    let b = b.map(f64::from);
    let intersection = ((a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0])).max(0.)
        * ((a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1])).max(0.);
    (intersection / (a[2] * a[3] + b[2] * b[3] - intersection)) as f32
}

/// Least-squares orientation-preserving similarity fit to all five points.
/// Inverse mapping uses bilinear sampling and zero outside the source image.
pub fn align_crop(image: &RgbImage, face: &Face) -> Result<RgbImage> {
    validate(face)?;
    ensure!(image.width() > 0 && image.height() > 0, "empty image");
    let mean = |points: &[[f32; 2]; 5]| {
        [
            points.iter().map(|p| f64::from(p[0])).sum::<f64>() / 5.,
            points.iter().map(|p| f64::from(p[1])).sum::<f64>() / 5.,
        ]
    };
    let src = mean(&face.landmarks5);
    let dst = mean(&CANONICAL);
    let (mut denominator, mut a, mut b) = (0., 0., 0.);
    for (p, q) in face.landmarks5.iter().zip(CANONICAL) {
        let (x, y) = (f64::from(p[0]) - src[0], f64::from(p[1]) - src[1]);
        let (u, v) = (f64::from(q[0]) - dst[0], f64::from(q[1]) - dst[1]);
        denominator += x * x + y * y;
        a += x * u + y * v;
        b += x * v - y * u;
    }
    ensure!(denominator > 1e-10, "degenerate landmarks");
    a /= denominator;
    b /= denominator;
    let determinant = a * a + b * b;
    ensure!(determinant > 1e-12, "degenerate alignment");
    Ok(RgbImage::from_fn(112, 112, |x, y| {
        let u = f64::from(x) - dst[0];
        let v = f64::from(y) - dst[1];
        sample(
            image,
            src[0] + (a * u + b * v) / determinant,
            src[1] + (-b * u + a * v) / determinant,
        )
    }))
}
fn sample(image: &RgbImage, x: f64, y: f64) -> Rgb<u8> {
    if x < -1. || y < -1. || x > f64::from(image.width()) || y > f64::from(image.height()) {
        return Rgb([0; 3]);
    }
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let (dx, dy) = (x - x.floor(), y - y.floor());
    let mut out = [0.; 3];
    for (px, py, weight) in [
        (ix, iy, (1. - dx) * (1. - dy)),
        (ix + 1, iy, dx * (1. - dy)),
        (ix, iy + 1, (1. - dx) * dy),
        (ix + 1, iy + 1, dx * dy),
    ] {
        if px >= 0 && py >= 0 && px < i64::from(image.width()) && py < i64::from(image.height()) {
            for (c, value) in out.iter_mut().enumerate() {
                *value += weight * f64::from(image.get_pixel(px as u32, py as u32)[c]);
            }
        }
    }
    Rgb(out.map(|v| v.round().clamp(0., 255.) as u8))
}

#[derive(Debug, Clone, Copy)]
pub struct FaceSignals {
    pub sharpness: f64,
    /// Weak geometry plausibility proxy, NOT eyelid aperture or blink probability.
    /// None means degenerate geometry. Never use as an automatic blink label.
    pub eyes_open: Option<f64>,
}
pub(crate) fn crop_rect((width, height): (u32, u32), face: &Face) -> Result<[u32; 4]> {
    validate(face)?;
    let [x, y, w, h] = face.bbox;
    let x0 = x.floor().max(0.) as u32;
    let y0 = y.floor().max(0.) as u32;
    let x1 = (x + w).ceil().clamp(0., width as f32) as u32;
    let y1 = (y + h).ceil().clamp(0., height as f32) as u32;
    ensure!(x1 > x0 && y1 > y0, "face outside image");
    Ok([x0, y0, x1 - x0, y1 - y0])
}

pub fn face_signals(image: &RgbImage, face: &Face) -> Result<FaceSignals> {
    let [x, y, w, h] = crop_rect(image.dimensions(), face)?;
    let crop = imageops::crop_imm(image, x, y, w, h).to_image();
    let sharpness = ml_quality::analyze(&crop)?.sharpness;
    let [right, left, nose, mouth_right, mouth_left] = face.landmarks5.map(|p| p.map(f64::from));
    let eye_distance = (left[0] - right[0]).hypot(left[1] - right[1]);
    let ex = (right[0] + left[0]) / 2.;
    let ey = (right[1] + left[1]) / 2.;
    let mouth_distance = ((mouth_right[0] + mouth_left[0]) / 2. - ex)
        .hypot((mouth_right[1] + mouth_left[1]) / 2. - ey);
    let nose_distance = (nose[0] - ex).hypot(nose[1] - ey);
    let eyes_open = if eye_distance > 1e-6 && mouth_distance > 1e-6 {
        let ratio = eye_distance / mouth_distance;
        let symmetry = 1.
            - (((nose[0] - right[0]).hypot(nose[1] - right[1])
                - (nose[0] - left[0]).hypot(nose[1] - left[1]))
            .abs()
                / eye_distance)
                .min(1.);
        Some(
            (1. - (ratio - 0.85).abs() / 0.85).clamp(0., 1.)
                * symmetry
                * (nose_distance / mouth_distance * 2.).clamp(0., 1.),
        )
    } else {
        None
    };
    Ok(FaceSignals {
        sharpness,
        eyes_open,
    })
}
