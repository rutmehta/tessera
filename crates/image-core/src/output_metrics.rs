//! Full-resolution SDR output measurements, after gamut mapping and U8 quantization.
//! These are independent of monitor/print proof profiles and EDR headroom.

/// Integer counts are merged on the host, never accumulated in float atomics.
/// Display luma bins use floor((2126 R + 7152 G + 722 B) * 256 / 2550000).
#[derive(Clone, Debug, PartialEq)]
pub struct OutputMetrics {
    pub pixels: u64,
    pub histogram: [[u64; 256]; 4],
    /// 256 uniform bins of linear-sRGB Rec.709 luminance (for quantiles/contrast).
    pub linear_histogram: [u64; 256],
    /// Number of pixels with ANY channel at 0 / 255, not the sum of channel counts.
    pub clipped_shadows: u64,
    pub clipped_highlights: u64,
}
impl Default for OutputMetrics {
    fn default() -> Self {
        Self {
            pixels: 0,
            histogram: [[0; 256]; 4],
            linear_histogram: [0; 256],
            clipped_shadows: 0,
            clipped_highlights: 0,
        }
    }
}
pub fn srgb_linear(v: u8) -> f64 {
    let x = f64::from(v) / 255.;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
impl OutputMetrics {
    /// Lossless conversion to the existing engine-api tool schema.
    pub fn display_histogram(&self) -> engine_api::EngineResult<engine_api::tools::Histogram> {
        let channel = |c: usize| {
            self.histogram[c]
                .iter()
                .map(|n| {
                    u32::try_from(*n).map_err(|_| {
                        engine_api::EngineError::invalid("histogram", "bin count exceeds u32")
                    })
                })
                .collect::<engine_api::EngineResult<Vec<_>>>()
        };
        Ok(engine_api::tools::Histogram {
            red: channel(0)?,
            green: channel(1)?,
            blue: channel(2)?,
            luminance: channel(3)?,
            clipped_shadows: self.shadow_fraction() as f32,
            clipped_highlights: self.highlight_fraction() as f32,
        })
    }
    /// CPU reference/fallback, on exactly the same encoded pixels as the GPU pass.
    pub fn add_pixel(&mut self, rgb: [u8; 3]) {
        self.pixels += 1;
        for (c, v) in rgb.iter().enumerate() {
            self.histogram[c][usize::from(*v)] += 1;
        }
        let [r, g, b] = rgb.map(u32::from);
        let y = (((2126 * r + 7152 * g + 722 * b) * 256) / 2_550_000).min(255);
        self.histogram[3][y as usize] += 1;
        let p = rgb.map(srgb_linear);
        let linear = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        self.linear_histogram[((linear * 256.) as usize).min(255)] += 1;
        self.clipped_shadows += u64::from(rgb.contains(&0));
        self.clipped_highlights += u64::from(rgb.contains(&255));
    }
    /// Exact linear-light mean from marginal channel histograms: no GPU pow/sum drift.
    pub fn mean_luminance(&self) -> f64 {
        [0.2126, 0.7152, 0.0722]
            .iter()
            .enumerate()
            .map(|(c, weight)| {
                weight
                    * self.histogram[c]
                        .iter()
                        .enumerate()
                        .map(|(i, n)| srgb_linear(i as u8) * *n as f64)
                        .sum::<f64>()
            })
            .sum::<f64>()
            / self.pixels.max(1) as f64
    }
    pub fn shadow_fraction(&self) -> f64 {
        self.clipped_shadows as f64 / self.pixels.max(1) as f64
    }
    pub fn highlight_fraction(&self) -> f64 {
        self.clipped_highlights as f64 / self.pixels.max(1) as f64
    }
    /// Full-resolution quantiles, with at most one 1/256-wide bin uncertainty.
    pub fn percentiles(&self) -> [f64; 3] {
        [0.1, 0.5, 0.9].map(|q| {
            let rank = (q * self.pixels.saturating_sub(1) as f64).floor() as u64;
            let mut count = 0;
            for (i, n) in self.linear_histogram.iter().enumerate() {
                count += n;
                if count > rank {
                    return (i as f64 + 0.5) / 256.;
                }
            }
            0.
        })
    }
    /// Full-resolution standard deviation estimated from linear-luma bin centres.
    pub fn contrast(&self) -> f64 {
        let mean = self.mean_luminance();
        (self
            .linear_histogram
            .iter()
            .enumerate()
            .map(|(i, n)| ((i as f64 + 0.5) / 256. - mean).powi(2) * *n as f64)
            .sum::<f64>()
            / self.pixels.max(1) as f64)
            .sqrt()
    }
}
