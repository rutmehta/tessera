use anyhow::{Result, ensure};
use ml_runtime::Tensor;

/// Rotate a Bayer sensor to RGGB without interpolating samples. Counterclockwise
/// quarter turns: RGGB=0, GRBG=1, BGGR=2, GBRG=3. Odd edges replicate same-phase
/// sites before rotation; unpack discards padding. Masks use this identical map.
pub struct BayerPacking {
    width: usize,
    height: usize,
    turns: u8,
    packed_width: usize,
    packed_height: usize,
}
impl BayerPacking {
    pub fn new(width: usize, height: usize, turns: u8) -> Result<Self> {
        ensure!(
            width >= 2 && height >= 2 && turns < 4,
            "invalid Bayer extent/rotation"
        );
        ensure!(
            width
                .checked_add(1)
                .and_then(|w| height.checked_add(1).and_then(|h| w.checked_mul(h)))
                .is_some(),
            "Bayer extent overflow"
        );
        let (w, h) = (width.div_ceil(2), height.div_ceil(2));
        let (packed_width, packed_height) = if turns.is_multiple_of(2) {
            (w, h)
        } else {
            (h, w)
        };
        Ok(Self {
            width,
            height,
            turns,
            packed_width,
            packed_height,
        })
    }
    fn source(&self, x: usize, y: usize) -> (usize, usize) {
        let w = self.width.div_ceil(2) * 2;
        let h = self.height.div_ceil(2) * 2;
        match self.turns {
            0 => (x, y),
            1 => (w - 1 - y, x),
            2 => (w - 1 - x, h - 1 - y),
            _ => (y, h - 1 - x),
        }
    }
    pub fn pack(&self, sensor: &[f32]) -> Result<Tensor> {
        ensure!(
            sensor.len() == self.width * self.height && sensor.iter().all(|v| v.is_finite()),
            "invalid sensor plane"
        );
        let (w, h) = (self.packed_width, self.packed_height);
        let mut data = Vec::with_capacity(4 * w * h);
        for c in 0..4 {
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = self.source(x * 2 + c % 2, y * 2 + c / 2);
                    let sx = if sx >= self.width { sx - 2 } else { sx };
                    let sy = if sy >= self.height { sy - 2 } else { sy };
                    data.push(sensor[sy * self.width + sx]);
                }
            }
        }
        Tensor::new(4, h, w, data)
    }
    pub fn unpack(&self, packed: &Tensor) -> Result<Vec<f32>> {
        let (w, h) = (self.packed_width, self.packed_height);
        ensure!(packed.shape() == [1, 4, h, w], "packed shape mismatch");
        let mut sensor = vec![0.0; self.width * self.height];
        for c in 0..4 {
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = self.source(x * 2 + c % 2, y * 2 + c / 2);
                    if sx < self.width && sy < self.height {
                        sensor[sy * self.width + sx] = packed.data()[c * w * h + y * w + x];
                    }
                }
            }
        }
        Ok(sensor)
    }
}
