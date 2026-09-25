use crate::{BrownConrady, Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSample {
    pub focal: f64,
    pub aperture: f64,
    pub distance: f64,
    pub distortion: BrownConrady,
    /// Lensfun poly3/poly5 constant radial scale (Brown itself has unit scale).
    pub distortion_scale: f64,
    /// Native PTLens odd radius terms c*r + a*r³.
    pub radial_odd: [f64; 2],
    /// Axis multipliers from public coordinates into the profile radial metric.
    pub coordinate_scale: [f64; 2],
    /// Observed channel radius / green radius = c0 + c1*r² + c2*r⁴.
    pub ca_red: [f64; 3],
    pub ca_blue: [f64; 3],
    /// Relative illumination = 1 + v0*r² + v1*r⁴ + v2*r⁶.
    pub vignette: [f64; 3],
}
impl Default for CalibrationSample {
    fn default() -> Self {
        Self {
            focal: 50.,
            aperture: 4.,
            distance: 10.,
            distortion: BrownConrady::default(),
            distortion_scale: 1.,
            radial_odd: [0.; 2],
            coordinate_scale: [1.; 2],
            ca_red: [1., 0., 0.],
            ca_blue: [1., 0., 0.],
            vignette: [0.; 3],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraIdentity {
    pub maker: String,
    pub model: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub maker: String,
    pub model: String,
    /// None denotes a camera-independent profile, not an unknown camera match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraIdentity>,
    pub samples: Vec<CalibrationSample>,
}
impl CalibrationSample {
    /// Ideal-to-observed geometry including native Lensfun radial scale/odd terms.
    pub fn distort(&self, p: crate::Point) -> crate::Point {
        let c = [self.distortion.cx, self.distortion.cy];
        let p = [
            (p[0] - c[0]) * self.coordinate_scale[0],
            (p[1] - c[1]) * self.coordinate_scale[1],
        ];
        let m = BrownConrady {
            cx: 0.,
            cy: 0.,
            ..self.distortion
        };
        let q = m.distort(p);
        let x = p[0];
        let y = p[1];
        let r = x.hypot(y);
        let extra =
            self.distortion_scale - 1. + self.radial_odd[0] * r + self.radial_odd[1] * r * r * r;
        [
            c[0] + (q[0] + x * extra) / self.coordinate_scale[0],
            c[1] + (q[1] + y * extra) / self.coordinate_scale[1],
        ]
    }
}
impl Profile {
    /// Inverse-distance interpolation in normalized focal/aperture/diopter space.
    /// Exact samples are preserved; sparse databases are not extrapolated.
    pub fn sample(&self, focal: f64, aperture: f64, distance: f64) -> Option<CalibrationSample> {
        if self.validate().is_err()
            || ![focal, aperture, distance]
                .iter()
                .all(|x| x.is_finite() && *x > 0.)
        {
            return None;
        }
        let coord = |s: &CalibrationSample| [s.focal, s.aperture, 1. / s.distance];
        let mut q = [focal, aperture, 1. / distance];
        let mut ranges = [0.; 3];
        for (i, r) in ranges.iter_mut().enumerate() {
            let lo = self
                .samples
                .iter()
                .map(|s| coord(s)[i])
                .fold(f64::INFINITY, f64::min);
            let hi = self
                .samples
                .iter()
                .map(|s| coord(s)[i])
                .fold(f64::NEG_INFINITY, f64::max);
            q[i] = q[i].clamp(lo, hi);
            *r = (hi - lo).max(1e-12);
        }
        let mut weights = Vec::new();
        let mut total = 0.;
        for s in &self.samples {
            let c = coord(s);
            let d = (0..3)
                .filter(|i| ranges[*i] > 1e-11)
                .map(|i| ((c[i] - q[i]) / ranges[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            if d < 1e-12 {
                return Some(s.clone());
            }
            let w = 1. / d;
            weights.push(w);
            total += w;
        }
        let mut out = CalibrationSample {
            focal,
            aperture,
            distance,
            distortion_scale: 0.,
            coordinate_scale: [0.; 2],
            ca_red: [0.; 3],
            ca_blue: [0.; 3],
            ..Default::default()
        };
        for (s, w) in self.samples.iter().zip(weights) {
            let w = w / total;
            out.distortion.k1 += w * s.distortion.k1;
            out.distortion.k2 += w * s.distortion.k2;
            out.distortion.k3 += w * s.distortion.k3;
            out.distortion.p1 += w * s.distortion.p1;
            out.distortion.p2 += w * s.distortion.p2;
            out.distortion.cx += w * s.distortion.cx;
            out.distortion.cy += w * s.distortion.cy;
            out.distortion_scale += w * s.distortion_scale;
            for i in 0..2 {
                out.radial_odd[i] += w * s.radial_odd[i];
                out.coordinate_scale[i] += w * s.coordinate_scale[i];
            }
            for i in 0..3 {
                out.ca_red[i] += w * s.ca_red[i];
                out.ca_blue[i] += w * s.ca_blue[i];
                out.vignette[i] += w * s.vignette[i];
            }
        }
        Some(out)
    }
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() || self.samples.is_empty() {
            return Err(Error::Invalid("empty profile".into()));
        }
        for s in &self.samples {
            let b = s.distortion;
            if !s.coordinate_scale.iter().all(|x| x.is_finite() && *x > 0.)
                || ![s.focal, s.aperture, s.distance]
                    .iter()
                    .all(|x| x.is_finite() && *x > 0.)
                || ![b.k1, b.k2, b.k3, b.p1, b.p2, b.cx, b.cy, s.distortion_scale]
                    .iter()
                    .chain(s.radial_odd.iter())
                    .chain(s.ca_red.iter())
                    .chain(s.ca_blue.iter())
                    .chain(s.vignette.iter())
                    .all(|x| x.is_finite())
            {
                return Err(Error::Invalid("nonfinite/invalid calibration".into()));
            }
        }
        Ok(())
    }
}
pub fn save_user_profile(path: impl AsRef<Path>, profile: &Profile) -> Result<()> {
    profile.validate()?;
    std::fs::write(path, serde_json::to_vec_pretty(profile)?)?;
    Ok(())
}
pub fn load_user_profile(path: impl AsRef<Path>) -> Result<Profile> {
    let p: Profile = serde_json::from_slice(&std::fs::read(path)?)?;
    p.validate()?;
    Ok(p)
}
#[derive(Clone, Debug, Default)]
pub struct ProfileDatabase {
    pub profiles: Vec<Profile>,
}
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
impl ProfileDatabase {
    /// Load rectilinear Adobe LCP RDF. Fisheye models are deliberately rejected.
    pub fn from_lcp(xml: &str) -> Result<Self> {
        let root = parse(xml)?;
        let mut all = Vec::new();
        root.descendants("Description", &mut all);
        root.descendants("li", &mut all);
        let mut p = Profile {
            maker: root.value("LensMake"),
            model: root.value("LensPrettyName"),
            camera: if root.value("Make").is_empty() && root.value("Model").is_empty() {
                None
            } else {
                Some(CameraIdentity {
                    maker: root.value("Make"),
                    model: root.value("Model"),
                })
            },
            samples: Vec::new(),
        };
        if p.model.is_empty() {
            p.model = root.value("Lens")
        }
        for n in all {
            if n.attr("FocalLength").is_empty()
                && !n.children.iter().any(|c| c.name == "FocalLength")
            {
                continue;
            }
            if !n.value("FisheyeModel").is_empty()
                || n.children.iter().any(|c| c.name == "FisheyeModel")
            {
                return Err(Error::Invalid("fisheye LCP unsupported".into()));
            }
            let mut s = CalibrationSample {
                focal: n.value_num("FocalLength", 50.)?,
                aperture: n.value_num("ApertureValue", 4.)?,
                distance: n.value_num("FocusDistance", 10.)?,
                ..Default::default()
            };
            let mut models = Vec::new();
            n.descendants("PerspectiveModel", &mut models);
            if let Some(m) = models.first() {
                s.coordinate_scale = [
                    0.5 / m.value_num("FocalLengthX", 0.5)?,
                    0.5 / m.value_num("FocalLengthY", 0.5)?,
                ];
                s.distortion = BrownConrady {
                    k1: m.value_num("RadialDistortParam1", 0.)?,
                    k2: m.value_num("RadialDistortParam2", 0.)?,
                    k3: m.value_num("RadialDistortParam3", 0.)?,
                    p1: m.value_num("TangentialDistortParam1", 0.)?,
                    p2: m.value_num("TangentialDistortParam2", 0.)?,
                    cx: 2. * m.value_num("ImageXCenter", 0.5)? - 1.,
                    cy: 2. * m.value_num("ImageYCenter", 0.5)? - 1.,
                };
            }
            for (name, target) in [
                ("ChromaticRedGreenModel", &mut s.ca_red),
                ("ChromaticBlueGreenModel", &mut s.ca_blue),
            ] {
                let mut v = Vec::new();
                n.descendants(name, &mut v);
                if let Some(m) = v.first() {
                    *target = [
                        m.value_num("ScaleFactor", 1.)?,
                        m.value_num("RadialDistortParam1", 0.)?,
                        m.value_num("RadialDistortParam2", 0.)?,
                    ];
                }
            }
            let mut v = Vec::new();
            n.descendants("VignetteModel", &mut v);
            if let Some(m) = v.first() {
                s.vignette = [
                    m.value_num("VignetteModelParam1", 0.)?,
                    m.value_num("VignetteModelParam2", 0.)?,
                    m.value_num("VignetteModelParam3", 0.)?,
                ];
            }
            p.samples.push(s);
        }
        p.validate()?;
        Ok(Self { profiles: vec![p] })
    }

    pub fn find(&self, maker: &str, model: &str) -> Option<&Profile> {
        self.find_for_camera("", "", maker, model)
    }

    /// Match lens identity only within compatible camera calibrations. Camera
    /// identifiers are normalized, not edit-distance matched: R5 and R6 are not
    /// interchangeable. Empty lens maker means unavailable, not camera make.
    pub fn find_for_camera(
        &self,
        camera_maker: &str,
        camera_model: &str,
        maker: &str,
        model: &str,
    ) -> Option<&Profile> {
        let m = normalize(model);
        let maker = normalize(maker);
        let camera_maker = normalize(camera_maker);
        let camera_model = normalize(camera_model);
        if m.is_empty() {
            return None;
        }
        let mut candidates: Vec<_> = self
            .profiles
            .iter()
            .filter(|p| maker.is_empty() || p.maker.is_empty() || normalize(&p.maker) == maker)
            .filter(|p| {
                p.camera.as_ref().is_none_or(|c| {
                    (!c.maker.is_empty() || !c.model.is_empty())
                        && (c.maker.is_empty() || normalize(&c.maker) == camera_maker)
                        && (c.model.is_empty() || normalize(&c.model) == camera_model)
                })
            })
            .map(|p| {
                let n = normalize(&p.model);
                let mut row: Vec<usize> = (0..=n.len()).collect();
                for (i, a) in m.bytes().enumerate() {
                    let mut prev = row[0];
                    row[0] = i + 1;
                    for (j, b) in n.bytes().enumerate() {
                        let old = row[j + 1];
                        row[j + 1] = (row[j] + 1).min(old + 1).min(prev + usize::from(a != b));
                        prev = old
                    }
                }
                let score = 1. - row[n.len()] as f64 / m.len().max(n.len()) as f64;
                (p, score)
            })
            .filter(|(_, s)| *s >= 0.72)
            .collect();
        let rank = |a: &(&Profile, f64), b: &(&Profile, f64)| {
            a.1.total_cmp(&b.1)
                .then(a.0.camera.is_some().cmp(&b.0.camera.is_some()))
        };
        candidates.sort_by(rank);
        let best = candidates.pop()?;
        // Missing lens maker must not select an arbitrary equal-scoring profile.
        if candidates
            .last()
            .is_some_and(|other| rank(other, &best).is_eq())
        {
            return None;
        }
        Some(best.0)
    }
    pub fn from_lensfun(xml: &str) -> Result<Self> {
        let root = parse(xml)?;
        let mut lenses = Vec::new();
        root.descendants("lens", &mut lenses);
        let mut profiles = Vec::new();
        for lens in lenses {
            let mut p = Profile {
                maker: lens.text_of("maker"),
                model: lens.text_of("model"),
                samples: Vec::new(),
                camera: None,
            };
            let mut nodes = Vec::new();
            lens.descendants("distortion", &mut nodes);
            for n in nodes {
                let mut s = CalibrationSample {
                    focal: n.num("focal", 50.)?,
                    ..Default::default()
                };
                match n.attr("model") {
                    "ptlens" => {
                        let a = n.num("a", 0.)?;
                        let b = n.num("b", 0.)?;
                        let c = n.num("c", 0.)?;
                        s.distortion_scale = 1. - a - b - c;
                        s.distortion.k1 = b;
                        s.radial_odd = [c, a];
                    }
                    "poly3" => {
                        s.distortion.k1 = n.num("k1", 0.)?;
                        s.distortion_scale = 1. - s.distortion.k1;
                    }
                    "poly5" => {
                        s.distortion.k1 = n.num("k1", 0.)?;
                        s.distortion.k2 = n.num("k2", 0.)?;
                    }
                    "brown" => {
                        s.distortion.k1 = n.num("k1", 0.)?;
                        s.distortion.k2 = n.num("k2", 0.)?;
                        s.distortion.k3 = n.num("k3", 0.)?;
                        s.distortion.p1 = n.num("p1", 0.)?;
                        s.distortion.p2 = n.num("p2", 0.)?;
                    }
                    other => {
                        return Err(Error::Invalid(format!(
                            "unsupported lensfun distortion {other}"
                        )))
                    }
                }
                p.samples.push(s);
            }
            let mut tca = Vec::new();
            lens.descendants("tca", &mut tca);
            let mut vig = Vec::new();
            lens.descendants("vignetting", &mut vig);
            // Lensfun measures these components on independent capture grids.
            // Resample onto their union, never replace geometry with the vignette
            // grid or discard a TCA focal length absent from the distortion grid.
            let mut ca = Profile {
                samples: Vec::new(),
                ..p.clone()
            };
            for n in tca {
                if n.attr("model") != "linear" {
                    return Err(Error::Invalid(format!(
                        "unsupported lensfun TCA {}",
                        n.attr("model")
                    )));
                }
                ca.samples.push(CalibrationSample {
                    focal: n.num("focal", 50.)?,
                    ca_red: [n.num("kr", 1.)?, 0., 0.],
                    ca_blue: [n.num("kb", 1.)?, 0., 0.],
                    ..Default::default()
                });
            }
            let mut illumination = Profile {
                samples: Vec::new(),
                ..p.clone()
            };
            for n in vig {
                if n.attr("model") != "pa" {
                    return Err(Error::Invalid("unsupported lensfun vignette".into()));
                }
                illumination.samples.push(CalibrationSample {
                    focal: n.num("focal", 50.)?,
                    aperture: n.num("aperture", 4.)?,
                    distance: n.num("distance", 10.)?,
                    vignette: [n.num("k1", 0.)?, n.num("k2", 0.)?, n.num("k3", 0.)?],
                    ..Default::default()
                });
            }
            for component in [&p, &ca, &illumination] {
                if !component.samples.is_empty() {
                    component.validate()?;
                }
            }
            let mut focals: Vec<_> = p
                .samples
                .iter()
                .chain(&ca.samples)
                .chain(&illumination.samples)
                .map(|s| s.focal)
                .collect();
            focals.sort_by(f64::total_cmp);
            focals.dedup();
            let mut captures: Vec<_> = illumination
                .samples
                .iter()
                .map(|s| (s.aperture, s.distance))
                .collect();
            captures.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            captures.dedup();
            if captures.is_empty() {
                captures.push((4., 10.));
            }
            let mut samples = Vec::new();
            for focal in focals {
                for &(aperture, distance) in &captures {
                    let mut s = p.sample(focal, 4., 10.).unwrap_or_default();
                    s.focal = focal;
                    s.aperture = aperture;
                    s.distance = distance;
                    if let Some(c) = ca.sample(focal, 4., 10.) {
                        s.ca_red = c.ca_red;
                        s.ca_blue = c.ca_blue;
                    }
                    if let Some(v) = illumination.sample(focal, aperture, distance) {
                        s.vignette = v.vignette;
                    }
                    samples.push(s);
                }
            }
            p.samples = samples;
            p.validate()?;
            profiles.push(p);
        }
        if profiles.is_empty() {
            return Err(Error::Invalid("no lens profiles".into()));
        }
        Ok(Self { profiles })
    }
}
#[derive(Default, Debug)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn value(&self, key: &str) -> String {
        if let Some(v) = self.attrs.get(key) {
            return v.clone();
        }
        if self.name == key {
            return self.text.trim().into();
        }
        for c in &self.children {
            let v = c.value(key);
            if !v.is_empty() {
                return v;
            }
        }
        String::new()
    }
    fn value_num(&self, key: &str, default: f64) -> Result<f64> {
        let v = self.value(key);
        if v.is_empty() {
            return Ok(default);
        }
        v.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| Error::Invalid(format!("bad numeric {key}")))
    }

    fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map(String::as_str).unwrap_or("")
    }
    fn num(&self, key: &str, default: f64) -> Result<f64> {
        let s = self.attr(key);
        if s.is_empty() {
            return Ok(default);
        }
        s.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| Error::Invalid(format!("bad numeric {key}")))
    }
    fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        if self.name == name {
            out.push(self)
        }
        for c in &self.children {
            c.descendants(name, out)
        }
    }
    fn text_of(&self, name: &str) -> String {
        let mut n = Vec::new();
        self.descendants(name, &mut n);
        n.first()
            .map(|n| n.text.trim().to_string())
            .unwrap_or_default()
    }
}
fn local(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .rsplit(':')
        .next()
        .unwrap_or("")
        .to_string()
}
fn parse(xml: &str) -> Result<Node> {
    use quick_xml::{events::Event, Reader};
    let mut reader = Reader::from_str(xml);
    let mut stack = vec![Node::default()];
    loop {
        match reader
            .read_event()
            .map_err(|e| Error::Invalid(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) => {
                let empty =
                    xml.as_bytes().get(reader.buffer_position() as usize - 2) == Some(&b'/');
                let mut n = Node {
                    name: local(e.name().as_ref()),
                    ..Default::default()
                };
                for a in e.attributes() {
                    let a = a.map_err(|e| Error::Invalid(e.to_string()))?;
                    n.attrs.insert(
                        local(a.key.as_ref()),
                        a.decode_and_unescape_value(reader.decoder())
                            .map_err(|e| Error::Invalid(e.to_string()))?
                            .into_owned(),
                    );
                }
                if empty {
                    stack.last_mut().unwrap().children.push(n)
                } else {
                    stack.push(n)
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(Error::Invalid("unbalanced XML".into()));
                }
                let n = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(n)
            }
            Event::Text(t) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&t.decode().map_err(|e| Error::Invalid(e.to_string()))?),
            Event::GeneralRef(r) => {
                let s = r.decode().map_err(|e| Error::Invalid(e.to_string()))?;
                let v = match s.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => return Err(Error::Invalid("unsupported XML entity".into())),
                };
                stack.last_mut().unwrap().text.push_str(v)
            }
            Event::DocType(_) => return Err(Error::Invalid("DTD not supported".into())),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(Error::Invalid("truncated XML".into()));
    }
    Ok(stack.pop().unwrap())
}
