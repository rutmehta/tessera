//! Display-sRGB / D65 CIE Lab and half-pixel bilinear sampling.
pub(crate) fn rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let xyz = [
        (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047,
        0.2126729 * r + 0.7151522 * g + 0.0721750 * b,
        (0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883,
    ];
    let [x, y, z] = xyz.map(|v| {
        if v > 216.0 / 24389.0 {
            v.cbrt()
        } else {
            v * (24389.0 / 27.0) / 116.0 + 16.0 / 116.0
        }
    });
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

/// Unclamped RGB: callers perform the final gamut clamp, after restoring L.
pub(crate) fn lab_to_rgb([l, a, b]: [f32; 3]) -> [f32; 3] {
    let y = (l + 16.0) / 116.0;
    let [x, y, z] = [y + a / 500.0, y, y - b / 200.0].map(|v| {
        if v > 6.0 / 29.0 {
            v * v * v
        } else {
            (v - 16.0 / 116.0) * 108.0 / 841.0
        }
    });
    let x = x * 0.95047;
    let z = z * 1.08883;
    [
        3.2404542 * x - 1.5371385 * y - 0.4985314 * z,
        -0.969_266 * x + 1.8760108 * y + 0.0415560 * z,
        0.0556434 * x - 0.2040259 * y + 1.0572252 * z,
    ]
    .map(|v| {
        if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    })
}

pub(crate) fn pixel_count(w: usize, h: usize) -> Option<usize> {
    w.checked_mul(h)
        .filter(|&n| n > 0 && n <= isize::MAX as usize / std::mem::size_of::<[f32; 3]>())
}

/// Caller has checked nonempty shapes and source length. Half-pixel coordinates
/// match OpenCV INTER_LINEAR / align_corners=false, including border replication.
pub(crate) fn sample<const C: usize>(
    src: &[[f32; C]],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    dw: usize,
    dh: usize,
) -> [f32; C] {
    let sx = (((x as f64 + 0.5) * w as f64 / dw as f64) - 0.5).clamp(0.0, (w - 1) as f64);
    let sy = (((y as f64 + 0.5) * h as f64 / dh as f64) - 0.5).clamp(0.0, (h - 1) as f64);
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let dx = (sx - x0 as f64) as f32;
    let dy = (sy - y0 as f64) as f32;
    std::array::from_fn(|c| {
        let top = src[y0 * w + x0][c] * (1.0 - dx) + src[y0 * w + x1][c] * dx;
        let bottom = src[y1 * w + x0][c] * (1.0 - dx) + src[y1 * w + x1][c] * dx;
        top * (1.0 - dy) + bottom * dy
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bilinear_half_pixel_edges_and_checked_shapes() {
        let src = [[0.0], [1.0]];
        assert_eq!(sample(&src, 2, 1, 0, 0, 4, 1), [0.0]);
        assert_eq!(sample(&src, 2, 1, 1, 0, 4, 1), [0.25]);
        assert_eq!(sample(&src, 2, 1, 2, 0, 4, 1), [0.75]);
        assert_eq!(sample(&src, 2, 1, 3, 0, 4, 1), [1.0]);
        assert_eq!(
            sample(&[[0.0], [2.0], [4.0], [6.0]], 2, 2, 0, 0, 1, 1),
            [3.0]
        );
        assert_eq!(pixel_count(0, 1), None);
        assert_eq!(pixel_count(usize::MAX, 2), None);
        assert_eq!(pixel_count(2, 3), Some(6));
    }
    #[test]
    fn lab_reference_and_roundtrip() {
        let red = rgb_to_lab([1.0, 0.0, 0.0]);
        for (actual, expected) in red.into_iter().zip([53.2408, 80.0925, 67.2032]) {
            assert!((actual - expected).abs() < 0.002);
        }
        for rgb in [[0.0; 3], [1.0; 3], [0.2, 0.5, 0.8], [1.0, 0.0, 0.0]] {
            for (actual, expected) in lab_to_rgb(rgb_to_lab(rgb)).into_iter().zip(rgb) {
                assert!((actual - expected).abs() < 0.00002);
            }
        }
    }
}
