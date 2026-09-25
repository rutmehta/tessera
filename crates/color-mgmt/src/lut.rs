/// Red-fastest normalized SDR lattice. Values retain the CMM's float output.
#[derive(Debug)]
pub struct Lut3d {
    pub size: usize,
    pub values: Vec<[f32; 3]>,
}
impl Lut3d {
    /// Trilinear interpolation; input is clamped to the normalized LUT domain.
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        assert!(
            self.size >= 2 && self.values.len() == self.size.pow(3),
            "invalid LUT dimensions"
        );
        let p = rgb.map(|v| {
            if v.is_nan() {
                0.0
            } else {
                v.clamp(0., 1.) * (self.size - 1) as f32
            }
        });
        let lo = p.map(|v| (v as usize).min(self.size - 2));
        let f = std::array::from_fn::<_, 3, _>(|i| p[i] - lo[i] as f32);
        let mut out = [0.; 3];
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    let offset = [r, g, b];
                    let weight: f32 = (0..3)
                        .map(|i| if offset[i] == 0 { 1. - f[i] } else { f[i] })
                        .product();
                    let value =
                        self.values[((lo[2] + b) * self.size + lo[1] + g) * self.size + lo[0] + r];
                    for c in 0..3 {
                        out[c] += value[c] * weight;
                    }
                }
            }
        }
        out
    }
}
