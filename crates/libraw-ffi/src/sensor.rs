use crate::{bindings, CfaImage, RawFile};

/// LibRaw channels: R=0, G=1, B=2, second green=3.
/// Origin is the unrotated full raw plane, including sensor margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfaLayout {
    Bayer([[u8; 2]; 2]),
    XTrans([[u8; 6]; 6]),
    Unsupported,
}

impl CfaLayout {
    pub fn channel_at(&self, x: u32, y: u32) -> usize {
        match self {
            Self::Bayer(p) => p[y as usize % 2][x as usize % 2] as usize,
            Self::XTrans(p) => p[y as usize % 6][x as usize % 6] as usize,
            Self::Unsupported => 0,
        }
    }
}

impl RawFile {
    /// Sensor metadata without copying the potentially huge sample plane.
    pub fn sensor_info(&self) -> CfaImage {
        // The owned LibRaw handle remains live throughout these reads.
        unsafe {
            let data = &*self.raw;
            let sizes = &data.sizes;
            let layout = if data.idata.filters == 9 {
                CfaLayout::XTrans(data.idata.xtrans_abs.map(|row| row.map(|v| v as u8)))
            } else if data.idata.filters >= 1000 && data.idata.colors == 3 {
                CfaLayout::Bayer(std::array::from_fn(|y| {
                    std::array::from_fn(|x| {
                        bindings::libraw_COLOR(
                            self.raw,
                            ((y + 2 - sizes.top_margin as usize % 2) % 2) as i32,
                            ((x + 2 - sizes.left_margin as usize % 2) % 2) as i32,
                        ) as u8
                    })
                }))
            } else {
                CfaLayout::Unsupported
            };
            let mut black =
                std::array::from_fn(|c| data.color.black as f32 + data.color.cblack[c] as f32);
            // DNG repeat cells are additive offsets relative to the active origin.
            let cb = &data.color.cblack;
            let (rows, cols) = (cb[4] as usize, cb[5] as usize);
            if rows > 0 && cols > 0 && rows.saturating_mul(cols) <= cb.len() - 6 {
                let mut sum = [0f64; 4];
                let mut count = [0u32; 4];
                for y in 0..rows * 6 {
                    for x in 0..cols * 6 {
                        let c = layout
                            .channel_at(
                                x as u32 + sizes.left_margin as u32,
                                y as u32 + sizes.top_margin as u32,
                            )
                            .min(3);
                        sum[c] += f64::from(cb[6 + (y % rows) * cols + x % cols]);
                        count[c] += 1;
                    }
                }
                for c in 0..4 {
                    if count[c] > 0 {
                        black[c] += (sum[c] / f64::from(count[c])) as f32;
                    }
                }
            }
            // rgb_cam maps white-balanced camera RGB to linear sRGB. cam_xyz,
            // despite its name, maps XYZ -> camera, so copying it is incorrect.
            let xyz_rgb = [
                [0.412453, 0.357580, 0.180423],
                [0.212671, 0.715160, 0.072169],
                [0.019334, 0.119193, 0.950227],
            ];
            let matrix = std::array::from_fn(|r| {
                std::array::from_fn(|c| {
                    (0..3)
                        .map(|k| xyz_rgb[r][k] * data.color.rgb_cam[k][c])
                        .sum()
                })
            });
            let mut crop = [
                sizes.left_margin as u32,
                sizes.top_margin as u32,
                sizes.width as u32,
                sizes.height as u32,
            ];
            let inset = &sizes.raw_inset_crops[0];
            let candidate = [
                u32::from(inset.cleft),
                u32::from(inset.ctop),
                u32::from(inset.cwidth),
                u32::from(inset.cheight),
            ];
            if candidate[2] > 0
                && candidate[3] > 0
                && candidate[0] + candidate[2] <= sizes.raw_width as u32
                && candidate[1] + candidate[3] <= sizes.raw_height as u32
            {
                crop = candidate;
            }
            if data.idata.dng_version != 0 {
                let dc = data.color.dng_levels.default_crop.map(u32::from);
                if dc[2] > 0
                    && dc[3] > 0
                    && dc[0] + dc[2] <= sizes.width as u32
                    && dc[1] + dc[3] <= sizes.height as u32
                {
                    crop = [
                        sizes.left_margin as u32 + dc[0],
                        sizes.top_margin as u32 + dc[1],
                        dc[2],
                        dc[3],
                    ];
                }
            }
            CfaImage {
                width: sizes.raw_width as u32,
                height: sizes.raw_height as u32,
                data: Vec::new(),
                cfa_layout: layout,
                black,
                white: data.color.maximum,
                wb_coeffs: data.color.cam_mul,
                color_matrix: matrix,
                crop,
            }
        }
    }
}

pub(crate) fn c_string(bytes: &[std::ffi::c_char]) -> String {
    String::from_utf8_lossy(
        &bytes
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect::<Vec<_>>(),
    )
    .into_owned()
}

pub(crate) fn exif_orientation(flip: i32) -> u16 {
    // LibRaw transpose / horizontal / vertical bit flags, not EXIF numbers.
    [1, 2, 4, 3, 5, 8, 6, 7]
        .get(flip as usize)
        .copied()
        .unwrap_or(1)
}

pub(crate) fn opcode_has_gain_map(bytes: &[u8]) -> bool {
    let read = |offset: usize| {
        bytes
            .get(offset..offset + 4)
            .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
    };
    let Some(count) = read(0) else {
        return false;
    };
    let mut offset: usize = 4;
    for _ in 0..count {
        let Some(id) = read(offset) else {
            return false;
        };
        let Some(len) = read(offset + 12) else {
            return false;
        };
        let Some(end) = offset
            .checked_add(16)
            .and_then(|o| o.checked_add(len as usize))
        else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        if id == 9 {
            return true;
        } // DNG GainMap opcode
        offset = end;
    }
    false
}
