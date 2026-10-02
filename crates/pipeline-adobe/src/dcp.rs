//! Safe, dependency-free subset of the public DNG camera profile format.

use std::collections::BTreeMap;
#[path = "dcp_acr3.rs"]
mod acr3;
#[path = "dcp_embedded.rs"]
mod embedded;
pub use embedded::read_embedded_profile;

type Matrix = [[f64; 3]; 3];
#[derive(Debug, Clone)]
pub struct DcpProfile {
    color1: Matrix,
    temperature1: f64,
    second: Option<(Matrix, f64)>,
    forward: Option<(Matrix, Option<Matrix>)>,
    hue1: Option<Table>,
    hue2: Option<Table>,
    look: Option<Table>,
    tone: Option<Tone>,
    baseline_exposure: f32,
    exposure_offset: f32,
    auto_black: bool,
}

/// Selected camera neutral and interpolation temperature, resolved once per render.
#[derive(Clone, Copy)]
pub struct DcpWhiteBalance {
    temperature: f32,
    neutral: [f64; 3],
}

struct Reader<'a> {
    bytes: &'a [u8],
    be: bool,
}
impl<'a> Reader<'a> {
    fn slice(&self, p: usize, n: usize) -> Result<&'a [u8], String> {
        let end = p.checked_add(n).ok_or("DCP offset overflow")?;
        self.bytes
            .get(p..end)
            .ok_or_else(|| "Truncated DCP data".into())
    }
    fn u16(&self, p: usize) -> Result<u16, String> {
        let b: [u8; 2] = self.slice(p, 2)?.try_into().unwrap();
        Ok(if self.be {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }
    fn u32(&self, p: usize) -> Result<u32, String> {
        let b: [u8; 4] = self.slice(p, 4)?.try_into().unwrap();
        Ok(if self.be {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
}
#[derive(Debug)]
struct Field {
    kind: u16,
    values: Vec<f64>,
}
fn fields(bytes: &[u8]) -> Result<BTreeMap<u16, Field>, String> {
    let be = match bytes.get(..2) {
        Some(b"II") => false,
        Some(b"MM") => true,
        _ => return Err("Invalid TIFF byte order".into()),
    };
    let r = Reader { bytes, be };
    if !matches!(r.u16(2)?, 42 | 0x4352) {
        return Err("Not a TIFF/DCP header".into());
    }
    let start = r.u32(4)? as usize;
    if start < 8 {
        return Err("Invalid IFD offset".into());
    }
    let count = r.u16(start)? as usize;
    r.slice(start, 2 + count * 12 + 4)?;
    if r.u32(start + 2 + count * 12)? != 0 {
        return Err("Multiple IFDs are unsupported".into());
    }
    let mut result = BTreeMap::new();
    let mut total_values = 0usize;
    for i in 0..count {
        let p = start + 2 + 12 * i;
        let tag = r.u16(p)?;
        let kind = r.u16(p + 2)?;
        let n = r.u32(p + 4)? as usize;
        let size = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => return Err(format!("Unknown TIFF field type {kind}")),
        };
        let len = n.checked_mul(size).ok_or("DCP field size overflow")?;
        let offset = if len <= 4 {
            p + 8
        } else {
            r.u32(p + 8)? as usize
        };
        r.slice(offset, len)?;
        if matches!(tag, 52529 | 52530 | 52531 | 52532 | 52535 | 52537 | 52538) {
            return Err("Triple-illuminant profiles are unsupported".into());
        }
        // Unknown metadata is bounds-checked but not allocated or interpreted.
        if !matches!(
            tag,
            50721
                | 50722
                | 50778
                | 50779
                | 50937
                | 50938
                | 50939
                | 50940
                | 50964
                | 50965
                | 50981
                | 50982
                | 51107
                | 51108
                | 50730
                | 51109
                | 51110
        ) {
            continue;
        }
        total_values = total_values.checked_add(n).ok_or("DCP resource limit")?;
        if n > 3_000_000 || total_values > 6_000_000 {
            return Err("DCP field exceeds resource limit".into());
        }
        if result.contains_key(&tag) {
            return Err(format!("Duplicate DCP tag {tag}"));
        }
        let mut values = Vec::with_capacity(n);
        for j in 0..n {
            let q = offset + j * size;
            let v = match kind {
                3 => r.u16(q)? as f64,
                4 => r.u32(q)? as f64,
                5 => {
                    let d = r.u32(q + 4)?;
                    if d == 0 {
                        return Err("Zero rational denominator".into());
                    }
                    r.u32(q)? as f64 / d as f64
                }
                10 => {
                    let d = r.u32(q + 4)? as i32;
                    if d == 0 {
                        return Err("Zero rational denominator".into());
                    }
                    (r.u32(q)? as i32) as f64 / d as f64
                }
                11 => f32::from_bits(r.u32(q)?) as f64,
                _ => return Err(format!("Unsupported type for DCP tag {tag}")),
            };
            if !v.is_finite() {
                return Err(format!("Nonfinite DCP tag {tag}"));
            }
            values.push(v);
        }
        result.insert(tag, Field { kind, values });
    }
    Ok(result)
}
fn required(f: &BTreeMap<u16, Field>, tag: u16, kind: u16, count: usize) -> Result<&[f64], String> {
    let v = f
        .get(&tag)
        .ok_or_else(|| format!("Missing DCP tag {tag}"))?;
    if v.kind != kind || v.values.len() != count {
        return Err(format!("Wrong type/count for DCP tag {tag}"));
    }
    Ok(&v.values)
}
fn matrix(f: &BTreeMap<u16, Field>, tag: u16) -> Result<Matrix, String> {
    let v = required(f, tag, 10, 9)?;
    let m = [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]];
    if inverse(m).is_none() {
        return Err(format!("Singular or ill-conditioned matrix {tag}"));
    }
    Ok(m)
}
fn illuminant(value: f64) -> Result<f64, String> {
    match value as u16 {
        1 | 4 | 9 => Ok(5500.),
        3 | 17 => Ok(2856.),
        10 => Ok(6504.),
        11 => Ok(7504.),
        18 => Ok(4874.),
        19 => Ok(6774.),
        20 => Ok(5503.),
        21 => Ok(6504.),
        22 => Ok(7504.),
        23 => Ok(5003.),
        24 => Ok(3200.),
        _ => Err("Unsupported calibration illuminant".into()),
    }
}
#[derive(Debug, Clone)]
struct Table {
    dims: [usize; 3],
    data: Vec<[f64; 3]>,
    encoded: bool,
}
impl Table {
    fn parse(
        f: &BTreeMap<u16, Field>,
        dims_tag: u16,
        data_tag: u16,
        encoding_tag: u16,
    ) -> Result<Option<Self>, String> {
        let encoding = if f.contains_key(&encoding_tag) {
            required(f, encoding_tag, 4, 1)?[0]
        } else {
            0.
        };
        if encoding != 0. && encoding != 1. {
            return Err("Unsupported table encoding".into());
        }
        if !f.contains_key(&dims_tag) && !f.contains_key(&data_tag) {
            return Ok(None);
        }
        let d = required(f, dims_tag, 4, 3)?;
        let dims = [d[0] as usize, d[1] as usize, d[2] as usize];
        if dims[0] < 1 || dims[1] < 2 || dims[2] < 1 {
            return Err("Invalid HSV table dimensions".into());
        }
        let n = dims
            .iter()
            .try_fold(3usize, |n, &d| n.checked_mul(d))
            .ok_or("HSV table size overflow")?;
        if n > 3_000_000 {
            return Err("HSV table exceeds resource limit".into());
        }
        let data = required(f, data_tag, 11, n)?
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if v[1] < 0. || v[2] < 0. {
                    return Err("Negative HSV table scale".into());
                }
                if i % dims[1] == 0 && (v[2] - 1.).abs() > 1e-6 {
                    return Err("Zero-saturation table value scale must be one".into());
                }
                Ok([v[0], v[1], v[2]])
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Self {
            dims,
            data,
            encoded: encoding == 1. && dims[2] > 1,
        }))
    }
    fn sample(&self, hsv: [f64; 3]) -> [f64; 3] {
        let [nh, ns, nv] = self.dims;
        let h = hsv[0].rem_euclid(360.) / 360. * nh as f64;
        let s = hsv[1].clamp(0., 1.) * (ns - 1) as f64;
        let v = hsv[2].clamp(0., 1.) * (nv - 1) as f64;
        let low = [h.floor() as usize, s.floor() as usize, v.floor() as usize];
        let high = [
            (low[0] + 1) % nh,
            (low[1] + 1).min(ns - 1),
            (low[2] + 1).min(nv - 1),
        ];
        let frac = [h - low[0] as f64, s - low[1] as f64, v - low[2] as f64];
        let mut out = [0.; 3];
        for mask in 0..8 {
            let mut idx = [0; 3];
            let mut weight = 1.;
            for axis in 0..3 {
                let upper = mask & (1 << axis) != 0;
                idx[axis] = if upper { high[axis] } else { low[axis] };
                weight *= if upper { frac[axis] } else { 1. - frac[axis] };
            }
            let cell = self.data[(idx[2] * nh + idx[0]) * ns + idx[1]];
            for c in 0..3 {
                out[c] += cell[c] * weight;
            }
        }
        out
    }
    fn apply(&self, rgb: [f64; 3], other: Option<&Table>, w: f64) -> [f64; 3] {
        let mut hsv = rgb_to_hsv(rgb.map(|v| v.max(0.)));
        if self.encoded {
            hsv[2] = encode(hsv[2]);
        }
        let mut adjustment = self.sample(hsv);
        if let Some(other) = other {
            let b = other.sample(hsv);
            for c in 0..3 {
                adjustment[c] = adjustment[c] * (1. - w) + b[c] * w;
            }
        }
        hsv[0] = (hsv[0] + adjustment[0]).rem_euclid(360.);
        hsv[1] = (hsv[1] * adjustment[1]).clamp(0., 1.);
        hsv[2] = (hsv[2] * adjustment[2]).clamp(0., 1.);
        if self.encoded {
            hsv[2] = decode(hsv[2]);
        }
        hsv_to_rgb(hsv)
    }
}
fn encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn rgb_to_hsv([r, g, b]: [f64; 3]) -> [f64; 3] {
    let v = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = v - min;
    if d <= 0. {
        return [0., 0., v];
    }
    let h = if v == r {
        (g - b) / d
    } else if v == g {
        2. + (b - r) / d
    } else {
        4. + (r - g) / d
    };
    [
        (h * 60.).rem_euclid(360.),
        if v > 0. { d / v } else { 0. },
        v,
    ]
}
fn hsv_to_rgb([h, s, v]: [f64; 3]) -> [f64; 3] {
    let h = h.rem_euclid(360.) / 60.;
    let c = v * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let m = v - c;
    let rgb = match h as usize {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    rgb.map(|a| a + m)
}
#[derive(Debug, Clone)]
struct Tone {
    points: Vec<[f64; 2]>,
    second: Vec<f64>,
}
impl Tone {
    fn parse(f: &BTreeMap<u16, Field>) -> Result<Option<Self>, String> {
        let Some(field) = f.get(&50940) else {
            return Ok(None);
        };
        let v = &field.values;
        if field.kind != 11 || v.len() < 4 || v.len() % 2 != 0 || v.len() > 131072 {
            return Err("Invalid tone curve size/type".into());
        }
        let points: Vec<[f64; 2]> = v.as_chunks::<2>().0.to_vec();
        if points[0] != [0., 0.]
            || *points.last().unwrap() != [1., 1.]
            || v.iter().any(|&v| !(0.0..=1.0).contains(&v))
        {
            return Err("Invalid tone curve endpoints/range".into());
        }
        if points.windows(2).any(|p| p[1][0] - p[0][0] < 1e-8) {
            return Err("Tone curve inputs must strictly increase".into());
        }
        let n = points.len();
        let mut diag = vec![0.; n];
        let mut rhs = vec![0.; n];
        let mut upper = vec![0.; n];
        diag[0] = 1.;
        diag[n - 1] = 1.;
        for i in 1..n - 1 {
            let a = points[i][0] - points[i - 1][0];
            let b = points[i + 1][0] - points[i][0];
            let k = a / diag[i - 1];
            diag[i] = 2. * (a + b) - k * upper[i - 1];
            upper[i] = b;
            rhs[i] = 6.
                * ((points[i + 1][1] - points[i][1]) / b - (points[i][1] - points[i - 1][1]) / a)
                - k * rhs[i - 1];
        }
        let mut second = vec![0.; n];
        for i in (1..n - 1).rev() {
            second[i] = (rhs[i] - upper[i] * second[i + 1]) / diag[i];
        }
        Ok(Some(Self { points, second }))
    }
    fn apply(&self, x: f64) -> f64 {
        let x = x.clamp(0., 1.);
        let i = self
            .points
            .partition_point(|p| p[0] <= x)
            .saturating_sub(1)
            .min(self.points.len() - 2);
        let h = self.points[i + 1][0] - self.points[i][0];
        let b = (x - self.points[i][0]) / h;
        let a = 1. - b;
        (a * self.points[i][1]
            + b * self.points[i + 1][1]
            + ((a * a * a - a) * self.second[i] + (b * b * b - b) * self.second[i + 1]) * h * h
                / 6.)
            .clamp(0., 1.)
    }
}
fn mul(m: Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
fn inverse(m: Matrix) -> Option<Matrix> {
    let mut cof = [[0.; 3]; 3];
    for (i, row) in cof.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3]
                - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3];
        }
    }
    let d = m[0][0] * cof[0][0] + m[0][1] * cof[0][1] + m[0][2] * cof[0][2];
    let scale = m.iter().flatten().fold(0.0_f64, |a, &v| a.max(v.abs()));
    if !d.is_finite() || d.abs() <= 1e-10 * scale.powi(3) {
        return None;
    }
    Some(std::array::from_fn(|i| {
        std::array::from_fn(|j| cof[j][i] / d)
    }))
}
fn mix(a: Matrix, b: Matrix, w: f64) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][j] * (1. - w) + b[i][j] * w))
}
const D50: [f64; 3] = [0.96422, 1., 0.82521];
// Rec.2020 D65 xy=(.3127,.3290), matching XYZ_TO_REC2020 below.
const D65: [f64; 3] = [0.9504559270516716, 1., 1.0890577507598784];
const XYZ_TO_PROPHOTO: Matrix = [
    [1.3459433, -0.2556075, -0.0511118],
    [-0.5445989, 1.5081673, 0.0205351],
    [0., 0., 1.2118128],
];
const PROPHOTO_TO_XYZ: Matrix = [
    [0.7976749, 0.1351917, 0.0313534],
    [0.2880402, 0.7118741, 0.0000857],
    [0., 0., 0.82521],
];
const XYZ_TO_REC2020: Matrix = [
    [1.716651188, -0.355670784, -0.253366281],
    [-0.666684352, 1.616481237, 0.015768546],
    [0.017639857, -0.042770613, 0.942103121],
];
fn white(t: f64) -> [f64; 3] {
    // Daylight locus above 4000 K, Planckian approximation below it.
    if (t - 6504.).abs() < 0.5 {
        return D65;
    }
    if (t - 5003.).abs() < 0.5 {
        return D50;
    }
    let x = if t >= 4000. {
        if t <= 7000. {
            -4.6070e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 99.11 / t + 0.244063
        } else {
            -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 247.48 / t + 0.237040
        }
    } else {
        -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 877.6956 / t + 0.179910
    };
    let y = if t >= 4000. {
        -3. * x * x + 2.87 * x - 0.275
    } else if t <= 2222. {
        -1.1063814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
    } else {
        -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
    };
    [x / y, 1., (1. - x - y) / y]
}
fn adapt(xyz: [f64; 3], from: [f64; 3], to: [f64; 3]) -> [f64; 3] {
    const B: Matrix = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    const BI: Matrix = [
        [0.986992905, -0.147054257, 0.159962652],
        [0.432305270, 0.518360272, 0.049291228],
        [-0.008528665, 0.040042822, 0.968486696],
    ];
    let a = mul(B, from);
    let b = mul(B, to);
    let c = mul(B, xyz);
    mul(BI, std::array::from_fn(|i| c[i] * b[i] / a[i]))
}
impl DcpProfile {
    /// Parse a standalone TIFF-style DCP profile. Supports one three-channel
    /// profile IFD, both byte orders, dual illuminants, forward matrices, HSV
    /// tables, and a tone curve. See `DCP.md` for the rendering contract and limits.
    /// Malformed or unsupported structural/color data returns a descriptive error.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let extracted;
        let bytes = if matches!(bytes.get(2..4), Some([42, 0] | [0, 42])) {
            extracted = read_embedded_profile(&mut std::io::Cursor::new(bytes))?
                .ok_or("No embedded camera profile")?;
            extracted.as_slice()
        } else {
            bytes
        };
        let f = fields(bytes)?;
        let temperature1 = illuminant(required(&f, 50778, 3, 1)?[0])?;
        let second = if f.contains_key(&50722) || f.contains_key(&50779) {
            let t = illuminant(required(&f, 50779, 3, 1)?[0])?;
            if (t - temperature1).abs() < 1. {
                return Err("Dual illuminants must differ".into());
            }
            Some((matrix(&f, 50722)?, t))
        } else {
            None
        };
        let forward = if f.contains_key(&50964) || f.contains_key(&50965) {
            let one = matrix(&f, 50964)?;
            let two = if second.is_some() {
                Some(matrix(&f, 50965)?)
            } else {
                if f.contains_key(&50965) {
                    return Err("ForwardMatrix2 requires dual illuminants".into());
                }
                None
            };
            for m in std::iter::once(one).chain(two) {
                let mapped = mul(m, [1.; 3]);
                if (0..3).any(|i| (mapped[i] - D50[i]).abs() > 0.002) {
                    return Err("ForwardMatrix must map unit camera neutral to D50".into());
                }
            }
            Some((one, two))
        } else {
            None
        };
        let hue1 = Table::parse(&f, 50937, 50938, 51107)?;
        let hue2 = if f.contains_key(&50939) {
            if second.is_none() {
                return Err("Second HueSatMap requires dual illuminants".into());
            }
            Table::parse(&f, 50937, 50939, 51107)?
        } else {
            None
        };
        let look = Table::parse(&f, 50981, 50982, 51108)?;
        let tone = Tone::parse(&f)?;
        let exposure = |tag| -> Result<f32, String> {
            let Some(field) = f.get(&tag) else {
                return Ok(0.);
            };
            if !matches!(field.kind, 5 | 10)
                || field.values.len() != 1
                || field.values[0].abs() > 32.
            {
                return Err(format!("Invalid exposure tag {tag}"));
            }
            Ok(field.values[0] as f32)
        };
        let black = if f.contains_key(&51110) {
            required(&f, 51110, 4, 1)?[0]
        } else {
            0.
        };
        if black != 0. && black != 1. {
            return Err("Unsupported DefaultBlackRender".into());
        }
        Ok(Self {
            baseline_exposure: exposure(50730)?,
            exposure_offset: exposure(51109)?,
            auto_black: black == 0.,
            color1: matrix(&f, 50721)?,
            temperature1,
            second,
            forward,
            hue1,
            hue2,
            look,
            tone,
        })
    }
    fn weight(&self, t: f64) -> f64 {
        self.second.map_or(0., |(_, t2)| {
            ((1. / t - 1. / self.temperature1) / (1. / t2 - 1. / self.temperature1)).clamp(0., 1.)
        })
    }
    /// Convert normalized, un-white-balanced camera RGB to linear Rec.2020 D65.
    /// Temperature is Kelvin; invalid values use CalibrationIlluminant1.
    pub fn apply(&self, rgb: [f32; 3], temperature: f32) -> [f32; 3] {
        self.apply_tone(self.apply_look(self.apply_exposure(
            self.apply_without_tone(rgb, temperature),
            self.baseline_exposure,
        )))
    }

    /// BaselineExposure + user exposure is supplied once by the source owner;
    /// the profile adds BaselineExposureOffset. Run after HueSatMap, before Look.
    /// Auto black uses the public SDK exposure ramp with shadows=5 and unit
    /// ShadowScale/Stage3Gain (black=.005). This is an explicit approximation,
    /// not Lightroom's image-dependent "shadows auto". DNG 1.7.1 pp. 62-63 leaves
    /// the amount/method reader-dependent. None bypasses subtraction entirely.
    pub fn apply_exposure(&self, rgb: [f32; 3], baseline_and_user: f32) -> [f32; 3] {
        let gain = f64::from((baseline_and_user + self.exposure_offset).clamp(-32., 32.)).exp2();
        if !self.auto_black {
            return rgb.map(|v| (f64::from(v) * gain) as f32);
        }
        let white = 1. / gain;
        let black = 0.005_f64.min(0.99 * white);
        let slope = 1. / (white - black);
        let radius = (0.5 * black).min(0.0625 / slope);
        map_prophoto(rgb, |pro| {
            pro.map(|x| {
                if x <= black - radius {
                    0.
                } else if x >= black + radius {
                    ((x - black) * slope).min(1.)
                } else {
                    slope / (4. * radius) * (x - black + radius).powi(2)
                }
            })
        })
    }

    /// Profile curve or the public SDK ACR3 default, in ProPhoto D50.
    /// The SDK's RefBaselineRGBTone maps channel extrema and interpolates the
    /// middle channel, preserving HSV hue (unlike three independent curves).
    /// https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_reference.cpp
    pub fn apply_tone(&self, rgb: [f32; 3]) -> [f32; 3] {
        map_prophoto(rgb, |pro| {
            let pro = pro.map(|v| v.clamp(0., 1.));
            let lo = pro.into_iter().fold(f64::INFINITY, f64::min);
            let hi = pro.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let curve = |v| {
                self.tone
                    .as_ref()
                    .map_or_else(|| acr3::evaluate(v), |t| t.apply(v))
            };
            let low = curve(lo);
            if hi <= lo {
                [low; 3]
            } else {
                let high = curve(hi);
                pro.map(|v| low + (high - low) * (v - lo) / (hi - lo))
            }
        })
    }

    /// ProfileLookTable is a separate post-exposure stage (DNG 1.7.1, p. 57).
    pub fn apply_look(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.look.as_ref().map_or(rgb, |table| {
            map_prophoto(rgb, |pro| table.apply(pro, None, 0.))
        })
    }

    /// The as-shot neutral is authoritative even off the temperature/tint slider
    /// locus. CCT is used only to interpolate profile calibrations and HueSatMaps.
    /// Custom tint uses Tessera's documented Duv convention, not Adobe's slider.
    pub fn resolve_white_balance(
        &self,
        settings: &engine_api::recipe::settings::WhiteBalanceSettings,
        multipliers: [f32; 4],
    ) -> engine_api::EngineResult<DcpWhiteBalance> {
        use engine_api::{EngineError, recipe::settings::WhiteBalanceMode};
        let normalize = |values: [f64; 3]| -> engine_api::EngineResult<[f64; 3]> {
            if values.iter().any(|v| !v.is_finite() || *v <= 0.) {
                return Err(EngineError::invalid(
                    "camera neutral",
                    "positive finite channels required",
                ));
            }
            let max = values.into_iter().fold(0., f64::max);
            Ok(values.map(|v| (v / max).clamp(0.001, 1.)))
        };
        if settings.mode == WhiteBalanceMode::AsShot {
            let neutral = normalize(std::array::from_fn(|i| 1. / f64::from(multipliers[i])))?;
            let mut temperature = self.temperature1;
            // Invert interpolated ColorMatrix and solve the nearest locus white
            // in CIE 1960 uv. DNG recommends iterative neutral-to-xy conversion;
            // this bounded nearest-locus CCT estimate approximates SDK Robertson.
            for _ in 0..12 {
                let cm = self.second.map_or(self.color1, |(m, _)| {
                    mix(self.color1, m, self.weight(temperature))
                });
                let xyz = mul(
                    inverse(cm).unwrap_or_else(|| inverse(self.color1).unwrap()),
                    neutral,
                );
                let uv = |v: [f64; 3]| {
                    let d = v[0] + 15. * v[1] + 3. * v[2];
                    [4. * v[0] / d, 6. * v[1] / d]
                };
                let target = uv(xyz);
                let distance = |t| {
                    let v = uv(white(t));
                    (v[0] - target[0]).powi(2) + (v[1] - target[1]).powi(2)
                };
                let mut best = (f64::INFINITY, temperature);
                for (low, high) in [(1667., 3999.999), (4000., 25000.)] {
                    let (mut lo, mut hi) = (1. / high, 1. / low);
                    for _ in 0..40 {
                        let a = lo + (hi - lo) / 3.;
                        let b = hi - (hi - lo) / 3.;
                        if distance(1. / a) < distance(1. / b) {
                            hi = b;
                        } else {
                            lo = a;
                        }
                    }
                    let t = 2. / (lo + hi);
                    let d = distance(t);
                    if d < best.0 {
                        best = (d, t);
                    }
                }
                if (best.1 - temperature).abs() < 0.01 {
                    temperature = best.1;
                    break;
                }
                temperature = best.1;
            }
            return Ok(DcpWhiteBalance {
                temperature: temperature as f32,
                neutral,
            });
        }
        let (temperature, tint) = match settings.mode {
            WhiteBalanceMode::Custom => (settings.temperature, settings.tint),
            WhiteBalanceMode::Daylight | WhiteBalanceMode::Flash => (5503., 0.),
            WhiteBalanceMode::Cloudy => (6504., 0.),
            WhiteBalanceMode::Shade => (7504., 0.),
            WhiteBalanceMode::Tungsten => (2856., 0.),
            WhiteBalanceMode::Fluorescent => (4230., 0.),
            _ => {
                return Err(EngineError::invalid(
                    "white balance",
                    "Auto is not implemented",
                ));
            }
        };
        let selected = if tint == 0. {
            white(f64::from(temperature))
        } else {
            let xy = pipeline_cpu::temperature_white(temperature, tint)?;
            [xy.x / xy.y, 1., (1. - xy.x - xy.y) / xy.y]
        };
        let cm = self.second.map_or(self.color1, |(m, _)| {
            mix(self.color1, m, self.weight(f64::from(temperature)))
        });
        // SDK dng_color_spec::SetWhiteXY normalizes CameraWhite to max=1.
        // https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_color_spec.cpp
        Ok(DcpWhiteBalance {
            temperature,
            neutral: normalize(mul(cm, selected))?,
        })
    }

    pub fn apply_camera(&self, rgb: [f32; 3], wb: &DcpWhiteBalance) -> [f32; 3] {
        self.apply_camera_at(rgb, wb.temperature, Some(wb.neutral))
    }

    /// Camera calibration, white balance and HueSatMap, before exposure,
    /// ProfileLookTable and ProfileToneCurve. Input is normalized unbalanced camera RGB.
    pub fn apply_without_tone(&self, rgb: [f32; 3], temperature: f32) -> [f32; 3] {
        self.apply_camera_at(rgb, temperature, None)
    }

    fn apply_camera_at(
        &self,
        rgb: [f32; 3],
        temperature: f32,
        selected_neutral: Option<[f64; 3]>,
    ) -> [f32; 3] {
        let t = if temperature.is_finite() && temperature > 0. {
            f64::from(temperature).clamp(1667., 25000.)
        } else {
            self.temperature1
        };
        let w = self.weight(t);
        let cm = self
            .second
            .map_or(self.color1, |(m, _)| mix(self.color1, m, w));
        // A valid endpoint pair can still cross a singular matrix. Fall back to
        // the nearer calibrated endpoint rather than generating NaNs.
        let inv = inverse(cm).unwrap_or_else(|| {
            inverse(if w > 0.5 {
                self.second.unwrap().0
            } else {
                self.color1
            })
            .unwrap()
        });
        let camera = rgb.map(|v| if v.is_finite() { f64::from(v) } else { 0. });
        let neutral = selected_neutral.unwrap_or_else(|| mul(cm, white(t)));
        let selected = mul(inv, neutral);
        let scale = if selected[1] > 1e-12 {
            1. / selected[1]
        } else {
            1.
        };
        let mut xyz = adapt(
            mul(inv, camera).map(|v| v * scale),
            selected.map(|v| v * scale),
            D50,
        );
        if let Some((one, two)) = self.forward
            && neutral.iter().all(|&v| v > 1e-12)
        {
            let fm = two.map_or(one, |two| mix(one, two, w));
            xyz = mul(fm, std::array::from_fn(|i| camera[i] / neutral[i]));
        }
        if self.hue1.is_some() {
            let mut pro = mul(XYZ_TO_PROPHOTO, xyz);
            if let Some(table) = &self.hue1 {
                pro = table.apply(pro, self.hue2.as_ref(), w);
            }
            xyz = mul(PROPHOTO_TO_XYZ, pro);
        }
        mul(XYZ_TO_REC2020, adapt(xyz, D50, D65))
            .map(|v| v.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32)
    }
}

fn map_prophoto(rgb: [f32; 3], op: impl FnOnce([f64; 3]) -> [f64; 3]) -> [f32; 3] {
    let xyz = mul(inverse(XYZ_TO_REC2020).unwrap(), rgb.map(f64::from));
    let pro = mul(XYZ_TO_PROPHOTO, adapt(xyz, D65, D50));
    let xyz = mul(PROPHOTO_TO_XYZ, op(pro));
    mul(XYZ_TO_REC2020, adapt(xyz, D50, D65)).map(|v| v as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lr10_missing_tone_uses_published_acr3_curve_but_explicit_identity_wins() {
        let p = DcpProfile::parse(&fixture(false, 42, &base())).unwrap();
        // Public DNG SDK ACR3 table samples at indices 128, 256, 512, 768.
        for (input, expected) in [
            (0.125, 0.25961),
            (0.25, 0.52069),
            (0.5, 0.80486),
            (0.75, 0.93986),
        ] {
            assert!((acr3::evaluate(f64::from(input)) - f64::from(expected)).abs() < 1e-7);
            // Rounded published color matrices add < 1e-4 through D50/D65.
            close(p.apply_tone([input; 3]), [expected; 3], 0.0001);
        }
        let mut entries = base();
        entries.push((50940, 11, vec![0., 0., 1., 1.]));
        let explicit = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        close(explicit.apply_tone([0.25; 3]), [0.25; 3], 0.00002);
    }

    #[test]
    fn lr10_look_is_deferred_until_after_exposure() {
        let mut entries = base();
        let plain = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        entries.extend([
            (50981, 4, vec![1., 2., 2.]),
            (
                50982,
                11,
                vec![0., 1., 1., 0., 1., 1., 0., 1., 1., 120., 1., 1.],
            ),
        ]);
        let look = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        let camera = [0.2, 0.1, 0.03];
        // The camera/WB/HueSat stage must not bake the value-dependent look.
        close(
            look.apply_without_tone(camera, 6504.),
            plain.apply_without_tone(camera, 6504.),
            0.00001,
        );
        assert_ne!(
            look.apply(camera, 6504.),
            plain.apply_without_tone(camera, 6504.)
        );
    }

    #[test]
    fn lr10_encoded_value_lookup_has_hand_computed_hue_and_value() {
        let table = Table {
            dims: [1, 2, 2],
            encoded: true,
            data: vec![[0., 1., 1.], [0., 1., 1.], [0., 1., 1.], [120., 1., 0.5]],
        };
        // Linear red V=0.21404114048223255 encodes to .5: interpolation
        // gives H+=60 degrees and V*=.75; decode(.375)=.11601613423276605.
        let out = table.apply([0.21404114048223255, 0., 0.], None, 0.);
        for (a, b) in out
            .into_iter()
            .zip([0.11601613423276605, 0.11601613423276605, 0.])
        {
            assert!((a - b).abs() < 1e-12, "{a} != {b}");
        }
    }

    #[test]
    fn lr10_profile_tone_preserves_hue_between_channel_extrema() {
        let mut entries = base();
        entries.push((50940, 11, vec![0., 0., 0.25, 0.1, 0.5, 0.3, 1., 1.]));
        let p = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        let to_working =
            |pro| mul(XYZ_TO_REC2020, adapt(mul(PROPHOTO_TO_XYZ, pro), D50, D65)).map(|v| v as f32);
        // Curve endpoints .25 -> .1, .5 -> .3. The middle channel lies
        // halfway between them, so hue preservation requires exactly .2.
        close(
            p.apply_tone(to_working([0.5, 0.375, 0.25])),
            to_working([0.3, 0.2, 0.1]),
            0.00002,
        );
    }

    fn neutral_profile() -> Vec<(u16, u16, Vec<f64>)> {
        let mut entries = base();
        entries[1].2 = vec![23.];
        entries.push((50940, 11, vec![0., 0., 1., 1.]));
        entries
    }

    #[test]
    fn lr10_baseline_and_profile_offset_are_added_once() {
        for kind in [5, 10] {
            let mut entries = neutral_profile();
            entries.extend([
                (50730, 10, vec![1.]),
                (51109, kind, vec![1.]),
                (51110, 4, vec![1.]),
            ]);
            let p = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
            close(
                p.apply([0.96422 * 0.05, 0.05, 0.82521 * 0.05], 5003.),
                [0.2; 3],
                0.0001,
            );
        }
    }

    #[test]
    fn lr10_auto_black_uses_sdk_shadow_ramp_and_none_bypasses_it() {
        let mut entries = neutral_profile();
        let auto = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        entries.push((51110, 4, vec![1.]));
        let none = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        let camera = [0.96422 * 0.02, 0.02, 0.82521 * 0.02];
        close(none.apply(camera, 5003.), [0.02; 3], 0.00001);
        // SDK defaults: shadows=5, shadowScale=stage3Gain=1; black=.005.
        // Above black+radius=.0075 the ramp is (x-.005)/(.995).
        close(auto.apply(camera, 5003.), [0.015075377; 3], 0.00001);
        entries.last_mut().unwrap().2 = vec![2.];
        assert!(DcpProfile::parse(&fixture(false, 42, &entries)).is_err());
    }

    #[test]
    fn lr10_camera_neutral_is_white_before_hue_tables_with_or_without_forward_matrix() {
        for forward in [false, true] {
            let mut entries = base();
            if forward {
                entries.push((
                    50964,
                    10,
                    vec![0.96422, 0., 0., 0., 1., 0., 0., 0., 0.82521],
                ));
            }
            let profile = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
            let wb = profile
                .resolve_white_balance(&Default::default(), [2., 1., 1.5, 1.])
                .unwrap();
            close(
                profile.apply_camera([0.1, 0.2, 0.13333334], &wb),
                [0.2; 3],
                0.00002,
            );
        }
    }

    #[test]
    fn lr10_exposure_runs_after_hue_and_before_value_dependent_look() {
        let mut entries = neutral_profile();
        let plain = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        let table = vec![0., 1., 1., 0., 1., 1., 0., 1., 1., 120., 1., 1.];
        entries.extend([
            (50730, 10, vec![1.]),
            (51110, 4, vec![1.]),
            (50937, 4, vec![1., 2., 2.]),
            (50938, 11, table.clone()),
            (50981, 4, vec![1., 2., 2.]),
            (50982, 11, table),
        ]);
        let p = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        // ProPhoto (.1,0,0): HueSat +12 degrees, exposure doubles V to .2,
        // Look +24 degrees => HSV(36,1,.2) => ProPhoto (.2,.12,0).
        let input = mul(PROPHOTO_TO_XYZ, [0.1, 0., 0.]).map(|v| v as f32);
        let expected = mul(PROPHOTO_TO_XYZ, [0.2, 0.12, 0.]).map(|v| v as f32);
        close(
            p.apply(input, 5003.),
            plain.apply_without_tone(expected, 5003.),
            0.00002,
        );
    }

    #[test]
    fn lr10_unknown_illuminant_uses_first_calibration_like_sdk() {
        let mut entries = base();
        entries[1].2 = vec![0.];
        let p = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        let known = DcpProfile::parse(&fixture(false, 42, &base())).unwrap();
        close(
            p.apply_without_tone([0.1, 0.2, 0.3], 5003.),
            known.apply_without_tone([0.1, 0.2, 0.3], 5003.),
            0.00001,
        );
    }

    #[test]
    fn lr10_output_referred_dng_has_no_implicit_tone_or_shadow_subtraction() {
        let mut entries = base();
        entries[1].2 = vec![23.];
        entries.push((50879, 3, vec![1.]));
        let p = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        close(p.apply_tone([0.25; 3]), [0.25; 3], 0.00001);
        close(
            p.apply([0.96422 * 0.02, 0.02, 0.82521 * 0.02], 5003.),
            [0.02; 3],
            0.00001,
        );
        // An explicit curve still overrides the output-referred default.
        entries.push((50940, 11, vec![0., 0., 0.5, 0.25, 1., 1.]));
        let explicit = DcpProfile::parse(&fixture(false, 42, &entries)).unwrap();
        close(explicit.apply_tone([0.5; 3]), [0.25; 3], 0.00002);
    }

    // Build actual TIFF IFDs, including out-of-line values, in either byte order.
    fn fixture(be: bool, magic: u16, entries: &[(u16, u16, Vec<f64>)]) -> Vec<u8> {
        fn u16b(v: u16, be: bool) -> [u8; 2] {
            if be { v.to_be_bytes() } else { v.to_le_bytes() }
        }
        fn u32b(v: u32, be: bool) -> [u8; 4] {
            if be { v.to_be_bytes() } else { v.to_le_bytes() }
        }
        let base = 8 + 2 + entries.len() * 12 + 4;
        let mut b = vec![0; base];
        b[..2].copy_from_slice(if be { b"MM" } else { b"II" });
        b[2..4].copy_from_slice(&u16b(magic, be));
        b[4..8].copy_from_slice(&u32b(8, be));
        b[8..10].copy_from_slice(&u16b(entries.len() as u16, be));
        for (i, (tag, typ, values)) in entries.iter().enumerate() {
            let p = 10 + i * 12;
            b[p..p + 2].copy_from_slice(&u16b(*tag, be));
            b[p + 2..p + 4].copy_from_slice(&u16b(*typ, be));
            b[p + 4..p + 8].copy_from_slice(&u32b(values.len() as u32, be));
            let mut data = Vec::new();
            for &v in values {
                match typ {
                    3 => data.extend(u16b(v as u16, be)),
                    4 => data.extend(u32b(v as u32, be)),
                    5 | 10 => {
                        data.extend(u32b((v * 1000000.0) as i32 as u32, be));
                        data.extend(u32b(1000000, be));
                    }
                    11 => data.extend(u32b((v as f32).to_bits(), be)),
                    _ => panic!("unsupported fixture type"),
                }
            }
            if data.len() <= 4 {
                b[p + 8..p + 8 + data.len()].copy_from_slice(&data);
            } else {
                let offset = b.len() as u32;
                b[p + 8..p + 12].copy_from_slice(&u32b(offset, be));
                b.extend(data);
            }
        }
        b
    }
    fn base() -> Vec<(u16, u16, Vec<f64>)> {
        vec![
            (50721, 10, vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
            (50778, 3, vec![21.]),
        ]
    }
    #[test]
    fn applies_huesat_then_look_then_tone_in_prophoto() {
        let mut e = base();
        e.push((51110, 4, vec![1.]));
        e[1].2 = vec![23.];
        let plain = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        e.extend([
            (50937, 4, vec![1., 2., 1.]),
            (50938, 11, vec![120., 1., 1., 120., 1., 1.]),
            (50981, 4, vec![3., 2., 1.]),
            (
                50982,
                11,
                vec![
                    0., 0., 1., 0., 0., 1., 0., 0.5, 1., 0., 0.5, 1., 0., 1., 1., 0., 1., 1.,
                ],
            ),
            (50940, 11, vec![0., 0., 0.25, 0.1, 0.5, 0.3, 1., 1.]),
        ]);
        let p = DcpProfile::parse(&fixture(false, 0x4352, &e)).unwrap();
        // ProPhoto red .5 -> green .5 -> [.25,.5,.25] -> [.1,.3,.1].
        let input = [0.7976749 * 0.5, 0.2880402 * 0.5, 0.];
        let expected = [
            0.7976749 * 0.1 + 0.1351917 * 0.3 + 0.0313534 * 0.1,
            0.2880402 * 0.1 + 0.7118741 * 0.3 + 0.0000857 * 0.1,
            0.82521 * 0.1,
        ];
        close(
            p.apply(input, 5003.),
            plain.apply_without_tone(expected, 5003.),
            0.0002,
        );
    }
    #[test]
    fn deferred_tone_matches_combined_apply() {
        let mut e = base();
        e.push((51110, 4, vec![1.]));
        let plain = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        e.push((50940, 11, vec![0., 0., 0.5, 0.25, 1., 1.]));
        let p = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        let camera = [0.2, 0.3, 0.1];
        let untoned = p.apply_without_tone(camera, 6504.);
        close(untoned, plain.apply_without_tone(camera, 6504.), 0.00001);
        close(p.apply_tone(untoned), p.apply(camera, 6504.), 0.00001);
        assert_ne!(p.apply_tone(untoned), untoned);
    }
    #[test]
    fn forward_matrix_uses_camera_neutral_and_maps_to_d50() {
        let mut e = base();
        e.push((
            50964,
            10,
            vec![0.96422, 0., 0., 0., 1., 0., 0., 0., 0.82521],
        ));
        let p = DcpProfile::parse(&fixture(false, 0x4352, &e)).unwrap();
        close(
            p.apply_without_tone([0.475235, 0.5, 0.544415], 6504.),
            [0.5; 3],
            0.0002,
        );
        // An FM intentionally different from the inverse-CM path must be used.
        let a = DcpProfile::parse(&fixture(false, 42, &base())).unwrap();
        assert!(
            (p.apply([0.2, 0.4, 0.1], 6504.)[0] - a.apply([0.2, 0.4, 0.1], 6504.)[0]).abs() > 0.001
        );
    }
    #[test]
    fn clips_table_value_as_required_by_dng() {
        let mut e = base();
        e[1].2 = vec![23.];
        e.extend([
            (50937, 4, vec![1., 2., 1.]),
            (50938, 11, vec![0., 1., 1., 0., 1., 2.]),
        ]);
        let p = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        let mut bare = base();
        bare[1].2 = vec![23.];
        let a = DcpProfile::parse(&fixture(false, 42, &bare)).unwrap();
        close(
            p.apply([0.7976749 * 0.75, 0.2880402 * 0.75, 0.], 5003.),
            a.apply([0.7976749, 0.2880402, 0.], 5003.),
            0.0001,
        );
    }
    #[test]
    fn rejects_unsupported_third_illuminant() {
        let mut e = base();
        e.push((52529, 3, vec![23.]));
        assert!(DcpProfile::parse(&fixture(false, 42, &e)).is_err());
    }
    #[test]
    fn malformed_offsets_counts_and_rationals_never_panic() {
        for be in [false, true] {
            let good = fixture(be, 0x4352, &base());
            for n in 0..good.len() {
                assert!(DcpProfile::parse(&good[..n]).is_err(), "prefix {n}");
            }
            for (start, end) in [(4, 8), (14, 18), (18, 22)] {
                let mut b = good.clone();
                b[start..end].fill(255);
                assert!(DcpProfile::parse(&b).is_err());
            }
            let mut b = good.clone();
            let offset = if be {
                u32::from_be_bytes(b[18..22].try_into().unwrap())
            } else {
                u32::from_le_bytes(b[18..22].try_into().unwrap())
            } as usize;
            b[offset + 4..offset + 8].fill(0);
            assert!(DcpProfile::parse(&b).is_err());
            let mut e = base();
            e.push(e[0].clone());
            assert!(DcpProfile::parse(&fixture(be, 42, &e)).is_err());
            let mut e = base();
            e[0].1 = 11;
            assert!(DcpProfile::parse(&fixture(be, 42, &e)).is_err());
        }
        let good = fixture(false, 42, &base());
        let mut seed = 0x12345678u32;
        for _ in 0..4000 {
            let mut b = good.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let i = seed as usize % b.len();
                b[i] = (seed >> 16) as u8;
            }
            if let Ok(p) = DcpProfile::parse(&b) {
                assert!(
                    p.apply([0.1, 0.2, 0.3], 5000.)
                        .iter()
                        .all(|v| v.is_finite())
                );
            }
        }
    }
    #[test]
    fn table_sampling_wraps_hue_and_interpolates_all_axes() {
        let table = Table {
            dims: [2, 2, 2],
            encoded: false,
            data: vec![
                [0., 1., 1.],
                [20., 2., 1.],
                [40., 1., 1.],
                [60., 2., 1.],
                [80., 1., 1.],
                [100., 2., 1.],
                [120., 1., 1.],
                [140., 2., 1.],
            ],
        };
        assert_eq!(table.sample([90., 0.5, 0.5]), [70., 1.5, 1.]);
        assert_eq!(table.sample([270., 0.5, 0.5]), [70., 1.5, 1.]);
        assert_eq!(table.sample([360., 0., 0.]), [0., 1., 1.]);
    }
    #[test]
    fn encoded_value_and_dual_hue_tables_are_used() {
        let mut e = base();
        e[1].2 = vec![23.];
        e.extend([
            (50937, 4, vec![1., 2., 2.]),
            (
                50938,
                11,
                vec![0., 1., 1., 0., 1., 1., 0., 1., 1., 120., 1., 1.],
            ),
            (51107, 4, vec![1.]),
        ]);
        let p = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        let mut linear = e.clone();
        linear.last_mut().unwrap().2 = vec![0.];
        let a = DcpProfile::parse(&fixture(false, 42, &linear)).unwrap();
        let input = [0.7976749 * 0.25, 0.2880402 * 0.25, 0.];
        let out = p.apply(input, 5003.);
        let lin = a.apply(input, 5003.);
        assert!((out[0] - lin[0]).abs() > 0.02);
        // Equal tables give the same result even in a dual-illuminant profile.
        e.push((50722, 10, base()[0].2.clone()));
        e.push((50779, 3, vec![17.]));
        let mut second = e[3].clone();
        second.0 = 50939;
        e.push(second);
        let dual = DcpProfile::parse(&fixture(true, 42, &e)).unwrap();
        close(dual.apply(input, 4000.), p.apply(input, 4000.), 0.0001);
        // Replacing Data2 must change the low-temperature endpoint only.
        e.last_mut().unwrap().2 = vec![0., 1., 1., 0., 1., 1., 0., 1., 1., 0., 1., 1.];
        let changed = DcpProfile::parse(&fixture(true, 42, &e)).unwrap();
        close(
            changed.apply(input, 5003.),
            dual.apply(input, 5003.),
            0.0001,
        );
        assert!((changed.apply(input, 2856.)[0] - dual.apply(input, 2856.)[0]).abs() > 0.01);
    }
    #[test]
    fn tone_is_cubic_and_invalid_input_is_finite() {
        let mut e = base();
        e.push((50940, 11, vec![0., 0., 0.5, 0.25, 1., 1.]));
        let p = DcpProfile::parse(&fixture(false, 42, &e)).unwrap();
        // Natural cubic, not piecewise-linear (which would produce .125).
        assert!((p.tone.as_ref().unwrap().apply(0.25) - 0.078125).abs() < 1e-9);
        for t in [f32::NAN, f32::INFINITY, -1., 0., 1., f32::MAX] {
            assert!(
                p.apply([f32::NAN, f32::INFINITY, -f32::MAX], t)
                    .iter()
                    .all(|v| v.is_finite())
            );
        }
    }
    #[test]
    fn validates_tables_and_tone_curve() {
        for extra in [
            vec![(50937, 4, vec![1., 1., 1.]), (50938, 11, vec![0., 1., 1.])],
            vec![(50937, 4, vec![1., 2., 1.])],
            vec![(50938, 11, vec![0., 1., 1.])],
            vec![(50940, 11, vec![0., 0., 0.5, 0.4, 0.5, 0.6, 1., 1.])],
            vec![(50940, 11, vec![0., 0., 1., 0.9])],
            vec![(51108, 4, vec![2.])],
        ] {
            let mut e = base();
            e.extend(extra);
            assert!(
                DcpProfile::parse(&fixture(false, 42, &e)).is_err(),
                "accepted {e:?}"
            );
        }
    }
    fn close(a: [f32; 3], b: [f32; 3], eps: f32) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < eps, "{a:?} != {b:?}");
        }
    }
    #[test]
    fn inverts_color_matrix_into_linear_rec2020() {
        let p = DcpProfile::parse(&fixture(false, 0x4352, &base())).unwrap();
        // With identity ColorMatrix, camera RGB is XYZ D65.
        close(
            p.apply_without_tone([0.95047, 1., 1.08883], 6504.),
            [1.; 3],
            0.002,
        );
        close(
            p.apply_without_tone([0.636958, 0.262700, 0.], 6504.),
            [1., 0., 0.],
            0.002,
        );
    }
    #[test]
    fn dual_illuminants_interpolate_in_reciprocal_kelvin() {
        let mut e = base();
        e.push((50722, 10, vec![2., 0., 0., 0., 2., 0., 0., 0., 2.]));
        e.push((50779, 3, vec![17.]));
        let p = DcpProfile::parse(&fixture(true, 42, &e)).unwrap();
        let a = DcpProfile::parse(&fixture(false, 42, &base())).unwrap();
        let t = (2. / (1. / 6504. + 1. / 2856.)) as f32;
        close(
            p.apply([0.3, 0.4, 0.5], t),
            a.apply([0.2, 0.4 / 1.5, 0.5 / 1.5], t),
            0.0001,
        );
        close(
            p.apply([0.3, 0.4, 0.5], 6504.),
            a.apply([0.3, 0.4, 0.5], 6504.),
            0.0001,
        );
    }
    #[test]
    fn rejects_singular_or_incomplete_matrix_pairs() {
        let mut e = base();
        e[0].2 = vec![0.; 9];
        assert!(DcpProfile::parse(&fixture(false, 42, &e)).is_err());
        let mut e = base();
        e.push((50779, 3, vec![17.]));
        assert!(DcpProfile::parse(&fixture(false, 42, &e)).is_err());
    }
    #[test]
    fn parses_both_endians_and_header_magics() {
        for be in [false, true] {
            assert!(DcpProfile::parse(&fixture(be, 4352, &base())).is_err());
            for magic in [42, 0x4352] {
                let p = DcpProfile::parse(&fixture(be, magic, &base())).unwrap();
                let black = p.apply([0.; 3], 6504.);
                assert_eq!(black, [0.; 3]);
            }
        }
    }
}
