use crate::*;
use quick_xml::{Reader, events::Event};
/// Raw records retain selectors, linked/unlinked bits, and reserved bytes exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsdPathRecord(pub [u8; 26]);
impl PsdPathRecord {
    pub fn selector(&self) -> u16 {
        u16::from_be_bytes([self.0[0], self.0[1]])
    }
    pub fn read(bytes: &[u8]) -> Result<Self> {
        Ok(Self(bytes.try_into().map_err(|_| {
            Error::Invalid("path record requires 26 bytes")
        })?))
    }
    pub fn write(&self) -> [u8; 26] {
        self.0
    }
    fn new(selector: u16) -> Self {
        let mut r = Self([0; 26]);
        r.0[..2].copy_from_slice(&selector.to_be_bytes());
        r
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsdVectorMask {
    pub flags: u32,
    pub records: Vec<PsdPathRecord>,
}
impl PsdVectorMask {
    /// vmsk/vsms payload including version 3 and flags, followed by 26-byte records.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 8
            || data[..4] != 3_u32.to_be_bytes()
            || !(data.len() - 8).is_multiple_of(26)
            || data.len() > 26_000_008
        {
            return Err(Error::Invalid("vector mask payload"));
        }
        Ok(Self {
            flags: u32::from_be_bytes(data[4..8].try_into().unwrap()),
            records: data[8..]
                .as_chunks::<26>()
                .0
                .iter()
                .map(|r| PsdPathRecord(*r))
                .collect(),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut out = 3_u32.to_be_bytes().to_vec();
        out.extend(self.flags.to_be_bytes());
        for r in &self.records {
            out.extend(r.write());
        }
        out
    }
    /// Coordinates are normalized document coordinates, not pixels.
    /// PSD paths use even-odd fill; nonzero geometry must first be normalized.
    pub fn from_path(path: &Path, flags: u32) -> Result<Self> {
        path.validate()?;
        if path.fill_rule != FillRule::EvenOdd {
            return Err(Error::Invalid("PSD export requires even-odd path"));
        }
        let mut records = vec![PsdPathRecord::new(6), PsdPathRecord::new(8)];
        for s in &path.subpaths {
            let count =
                u16::try_from(s.anchors.len()).map_err(|_| Error::Invalid("too many PSD knots"))?;
            let mut length = PsdPathRecord::new(if s.closed { 0 } else { 3 });
            length.0[2..4].copy_from_slice(&count.to_be_bytes());
            records.push(length);
            for a in &s.anchors {
                let mut r = PsdPathRecord::new(if s.closed { 2 } else { 5 });
                for (i, v) in [
                    a.incoming.y,
                    a.incoming.x,
                    a.point.y,
                    a.point.x,
                    a.outgoing.y,
                    a.outgoing.x,
                ]
                .into_iter()
                .enumerate()
                {
                    let fixed = (v * 16777216.).round();
                    if fixed < f64::from(i32::MIN) || fixed > f64::from(i32::MAX) {
                        return Err(Error::Invalid("PSD 8.24 coordinate range"));
                    }
                    r.0[2 + i * 4..6 + i * 4].copy_from_slice(&(fixed as i32).to_be_bytes());
                }
                records.push(r);
            }
        }
        Ok(Self { flags, records })
    }
    /// Geometry only: flags and initial-fill inversion are available separately.
    /// Unknown records are retained by decode/encode but rejected for rendering.
    pub fn path(&self) -> Result<Path> {
        let mut p = Path::default().with_rule(FillRule::EvenOdd);
        let mut index = 0;
        while index < self.records.len() {
            let r = &self.records[index];
            match r.selector() {
                0 | 3 => {
                    let closed = r.selector() == 0;
                    let count = usize::from(u16::from_be_bytes([r.0[2], r.0[3]]));
                    let mut anchors = vec![];
                    for _ in 0..count {
                        index += 1;
                        let r = self
                            .records
                            .get(index)
                            .ok_or(Error::Invalid("truncated PSD subpath"))?;
                        if !(if closed { [1, 2] } else { [4, 5] }).contains(&r.selector()) {
                            return Err(Error::Invalid("PSD knot topology"));
                        }
                        let values: Vec<_> = r.0[2..]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| f64::from(i32::from_be_bytes(*b)) / 16777216.)
                            .collect();
                        anchors.push(Anchor {
                            incoming: Point::new(values[1], values[0]),
                            point: Point::new(values[3], values[2]),
                            outgoing: Point::new(values[5], values[4]),
                        });
                    }
                    p.subpaths.push(Subpath { anchors, closed });
                }
                6 | 7 => {}
                8 => {
                    if u16::from_be_bytes([r.0[2], r.0[3]]) > 1 {
                        return Err(Error::Invalid("PSD initial fill"));
                    }
                }
                _ => return Err(Error::Invalid("unsupported or orphan PSD path record")),
            }
            index += 1;
        }
        Ok(p)
    }
    pub fn initially_filled(&self) -> bool {
        self.records
            .iter()
            .any(|r| r.selector() == 8 && r.0[2..4] == [0, 1])
    }
    pub fn coverage(
        &self,
        renderer: &VectorRenderer,
        document_size: Vec2,
        view: Viewport,
    ) -> Result<CoverageRaster> {
        if !document_size.x.is_finite()
            || !document_size.y.is_finite()
            || document_size.x <= 0.
            || document_size.y <= 0.
        {
            return Err(Error::Invalid("PSD document size"));
        }
        let path = self
            .path()?
            .affine(Affine::scale_non_uniform(document_size.x, document_size.y));
        let mut raster = renderer.coverage(&path, view)?;
        if self.flags & 4 != 0 {
            raster.data.fill(1.);
        } else if (self.flags & 1 != 0) ^ self.initially_filled() {
            for v in &mut raster.data {
                *v = 1. - *v;
            }
        }
        Ok(raster)
    }
}
impl Path {
    pub fn from_svg_data(data: &str) -> Result<Self> {
        let b = kurbo::BezPath::from_svg(data).map_err(|_| Error::Invalid("SVG path data"))?;
        let p = Self::from_bez(&b);
        p.validate()?;
        Ok(p)
    }
    pub fn to_svg_data(&self) -> String {
        self.to_bez().to_svg()
    }
    pub fn to_svg(&self) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill-rule=\"{}\" d=\"{}\"/></svg>",
            if self.fill_rule == FillRule::EvenOdd {
                "evenodd"
            } else {
                "nonzero"
            },
            self.to_svg_data()
        )
    }
    /// Basic single-path SVG. Rejects styling/transforms rather than silently losing them.
    pub fn from_svg(svg: &str) -> Result<Self> {
        let mut reader = Reader::from_str(svg);
        reader.config_mut().expand_empty_elements = true;
        let mut depth = 0;
        let mut finished = false;
        let mut path = None;
        loop {
            match reader.read_event().map_err(|_| Error::Invalid("SVG XML"))? {
                Event::Start(e) => {
                    if finished || depth > 1 {
                        return Err(Error::Invalid("SVG nesting"));
                    }
                    depth += 1;
                    match e.name().as_ref() {
                        b"svg" => {
                            if depth != 1 {
                                return Err(Error::Invalid("nested SVG"));
                            }
                            for a in e.attributes() {
                                let a = a.map_err(|_| Error::Invalid("SVG attribute"))?;
                                if a.key.as_ref() != b"xmlns" {
                                    return Err(Error::Invalid(
                                        "unsupported SVG viewport attribute",
                                    ));
                                }
                            }
                        }
                        b"path" => {
                            if path.is_some() {
                                return Err(Error::Invalid("SVG expects one path"));
                            }
                            let mut data = None;
                            let mut rule = FillRule::NonZero;
                            for a in e.attributes() {
                                let a = a.map_err(|_| Error::Invalid("SVG attribute"))?;
                                let value = a
                                    .decode_and_unescape_value(reader.decoder())
                                    .map_err(|_| Error::Invalid("SVG attribute value"))?;
                                match a.key.as_ref() {
                                    b"d" => data = Some(value.into_owned()),
                                    b"fill-rule" => {
                                        rule = match value.as_ref() {
                                            "evenodd" => FillRule::EvenOdd,
                                            "nonzero" => FillRule::NonZero,
                                            _ => return Err(Error::Invalid("SVG fill rule")),
                                        }
                                    }
                                    _ => {
                                        return Err(Error::Invalid(
                                            "unsupported SVG path attribute",
                                        ));
                                    }
                                }
                            }
                            path = Some(
                                Self::from_svg_data(&data.ok_or(Error::Invalid("missing SVG d"))?)?
                                    .with_rule(rule),
                            );
                        }
                        _ => return Err(Error::Invalid("unsupported SVG element")),
                    }
                }
                Event::End(_) => {
                    if depth == 0 {
                        return Err(Error::Invalid("SVG end tag"));
                    }
                    depth -= 1;
                    finished = depth == 0;
                }
                Event::Eof => {
                    if !finished || depth != 0 {
                        return Err(Error::Invalid("unclosed SVG"));
                    }
                    break;
                }
                Event::Text(t) if t.as_ref().iter().any(|b| !b.is_ascii_whitespace()) => {
                    return Err(Error::Invalid("SVG text unsupported"));
                }
                Event::DocType(_) => return Err(Error::Invalid("SVG doctype unsupported")),
                _ => {}
            }
        }
        path.ok_or(Error::Invalid("missing SVG path"))
    }
}
