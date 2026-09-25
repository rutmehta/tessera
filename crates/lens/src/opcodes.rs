//! Dependency-free DNG correction opcode parser. All opcode values are big-endian.

/// Per-plane [k0, k1, k2, k3, p1, p2] and normalized [cx, cy].
#[derive(Debug, Clone, PartialEq)]
pub struct WarpRectilinear {
    pub coefficients: Vec<[f64; 6]>,
    pub center: [f64; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub enum CorrectionOpcode {
    WarpRectilinear(WarpRectilinear),
    FixVignetteRadial(FixVignetteRadial),
}
/// Gain polynomial coefficients for r^2 through r^10, normalized center.
#[derive(Debug, Clone, PartialEq)]
pub struct FixVignetteRadial {
    pub coefficients: [f64; 5],
    pub center: [f64; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedOpcode {
    pub minimum_version: u32,
    pub flags: u32,
    pub correction: CorrectionOpcode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpcodeError(pub &'static str);
impl std::fmt::Display for OpcodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for OpcodeError {}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], OpcodeError> {
        let b = self.0.get(..n).ok_or(OpcodeError("truncated opcode"))?;
        self.0 = &self.0[n..];
        Ok(b)
    }
    fn center(&mut self) -> Result<[f64; 2], OpcodeError> {
        let center = self.doubles()?;
        if center.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(OpcodeError("center outside normalized image"));
        }
        Ok(center)
    }
    fn u32(&mut self) -> Result<u32, OpcodeError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn doubles<const N: usize>(&mut self) -> Result<[f64; N], OpcodeError> {
        let mut out = [0.; N];
        for x in &mut out {
            *x = f64::from_be_bytes(self.take(8)?.try_into().unwrap());
            if !x.is_finite() {
                return Err(OpcodeError("nonfinite coefficient"));
            }
        }
        Ok(out)
    }
}
/// Parse corrections only; other opcodes (including bad pixels) are skipped.
/// Preserves version/flags for the caller's application policy. Does not apply opcodes.
pub fn parse_opcode_list(bytes: &[u8]) -> Result<Vec<ParsedOpcode>, OpcodeError> {
    let mut r = Reader(bytes);
    let count = r.u32()? as usize;
    if count > r.0.len() / 16 {
        return Err(OpcodeError("invalid opcode count"));
    }
    let mut result = Vec::new();
    for _ in 0..count {
        let id = r.u32()?;
        let minimum_version = r.u32()?;
        let flags = r.u32()?;
        let length = r.u32()? as usize;
        let mut p = Reader(r.take(length)?);
        let correction = match id {
            1 => {
                let n = p.u32()? as usize;
                if !(1..=3).contains(&n) || p.0.len() != n * 48 + 16 {
                    return Err(OpcodeError("invalid warp planes/length"));
                }
                let mut coefficients = Vec::with_capacity(n);
                for _ in 0..n {
                    coefficients.push(p.doubles()?);
                }
                CorrectionOpcode::WarpRectilinear(WarpRectilinear {
                    coefficients,
                    center: p.center()?,
                })
            }
            3 => {
                if p.0.len() != 56 {
                    return Err(OpcodeError("invalid vignette length"));
                }
                CorrectionOpcode::FixVignetteRadial(FixVignetteRadial {
                    coefficients: p.doubles()?,
                    center: p.center()?,
                })
            }
            _ => continue,
        };
        result.push(ParsedOpcode {
            minimum_version,
            flags,
            correction,
        });
    }
    if !r.0.is_empty() {
        return Err(OpcodeError("trailing opcode bytes"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warp_three_planes_and_invalid_values() {
        let mut p = 3u32.to_be_bytes().to_vec();
        for v in [1.0f64; 18].into_iter().chain([0.5, 0.5]) {
            p.extend(v.to_be_bytes());
        }
        let mut b = list(1, &p);
        b[12..16].copy_from_slice(&3u32.to_be_bytes());
        let parsed = parse_opcode_list(&b).unwrap();
        assert_eq!(parsed[0].flags, 3);
        let CorrectionOpcode::WarpRectilinear(w) = &parsed[0].correction else {
            panic!()
        };
        assert_eq!(w.coefficients.len(), 3);
        for n in 0..b.len() {
            assert!(parse_opcode_list(&b[..n]).is_err());
        }
        let mut bad = p.clone();
        bad[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse_opcode_list(&list(1, &bad)).is_err());
        let mut bad = p.clone();
        bad[4..12].copy_from_slice(&f64::INFINITY.to_be_bytes());
        assert!(parse_opcode_list(&list(1, &bad)).is_err());
        let end = p.len();
        p[end - 8..].copy_from_slice(&(-1f64).to_be_bytes());
        assert!(parse_opcode_list(&list(1, &p)).is_err());
    }
    #[test]
    fn vignette_and_malformed() {
        let p: Vec<_> = [0.1f64, 0.2, 0.3, 0.4, 0.5, 0.45, 0.55]
            .into_iter()
            .flat_map(f64::to_be_bytes)
            .collect();
        let b = list(3, &p);
        assert_eq!(parse_opcode_list(&b).unwrap().len(), 1);
        for n in 0..b.len() {
            assert!(parse_opcode_list(&b[..n]).is_err());
        }
        assert!(parse_opcode_list(&list(3, &[0; 57])).is_err());
        let mut bad = p.clone();
        bad[..8].copy_from_slice(&f64::NAN.to_be_bytes());
        assert!(parse_opcode_list(&list(3, &bad)).is_err());
        for id in [4, 5, 9999] {
            assert!(parse_opcode_list(&list(id, &[1, 2, 3])).unwrap().is_empty());
        }
        assert!(parse_opcode_list(&[255; 20]).is_err());
        assert!(parse_opcode_list(&[0; 5]).is_err());
    }
    fn list(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        for n in [1, id, 0x01030000, 0, payload.len() as u32] {
            b.extend(n.to_be_bytes());
        }
        b.extend(payload);
        b
    }
    #[test]
    fn rectilinear_preserves_planes_and_center() {
        let mut p = 1u32.to_be_bytes().to_vec();
        for x in [1.0f64, 0.1, 0.2, 0.3, 0.04, 0.05, 0.5, 0.6] {
            p.extend(x.to_be_bytes());
        }
        let parsed = parse_opcode_list(&list(1, &p)).unwrap();
        let CorrectionOpcode::WarpRectilinear(w) = &parsed[0].correction else {
            panic!()
        };
        assert_eq!(w.coefficients, vec![[1.0, 0.1, 0.2, 0.3, 0.04, 0.05]]);
        assert_eq!(w.center, [0.5, 0.6]);
        assert_eq!(parsed[0].minimum_version, 0x01030000);
    }
}
