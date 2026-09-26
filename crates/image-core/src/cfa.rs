//! Packed CFA interchange. These are host f32 tensors, not shared GPU memory.
use engine_api::{EngineError, EngineResult, recipe::settings::DenoiseSettings, tile::Extent};
use pipeline_cpu::{Image, PostDemosaicDenoise};
use raw_decode::CfaLayout;

/// Optional capability alongside the legacy CPU denoiser. Inference returns
/// full-strength predictions; Amount and per-site coverage are applied later.
pub trait CfaDenoise: PostDemosaicDenoise {
    fn supports(&self, cfa: CfaLayout, settings: &DenoiseSettings) -> bool;
    fn infer(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
    ) -> EngineResult<PackedCfa>;
    /// Renderer identity includes ImageId and upstream graph state. Adapters
    /// with their own memo must include it, even for numerically equal inputs.
    fn infer_keyed(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
        _identity: engine_api::stage::MemoKey,
    ) -> EngineResult<PackedCfa> {
        self.infer(input, cfa, settings)
    }
}

pub fn bayer_turns(cfa: CfaLayout) -> Option<u8> {
    if !matches!(cfa, CfaLayout::Bayer(_)) {
        return None;
    }
    match [
        cfa.channel_at(0, 0),
        cfa.channel_at(1, 0),
        cfa.channel_at(0, 1),
        cfa.channel_at(1, 1),
    ]
    .map(|v| if v == 3 { 1 } else { v })
    {
        [0, 1, 1, 2] => Some(0),
        [1, 0, 2, 1] => Some(1),
        [2, 1, 1, 0] => Some(2),
        [1, 2, 0, 1] => Some(3),
        _ => None,
    }
}

#[derive(Clone)]
enum HostSamples {
    Array(std::sync::Arc<[f32]>),
    #[cfg(feature = "ml-denoise")]
    Tensor(std::sync::Arc<ml_runtime::Tensor>),
}
impl std::ops::Deref for HostSamples {
    type Target = [f32];
    fn deref(&self) -> &[f32] {
        match self {
            Self::Array(v) => v,
            #[cfg(feature = "ml-denoise")]
            Self::Tensor(v) => v.data(),
        }
    }
}

/// NCHW RGGB output after rotation of the entire even-padded sensor. Odd
/// quarter turns swap width/height. Coverage has the identical four-site map.
/// Private fields ensure backends only receive validated finite payloads.
#[derive(Clone)]
pub struct PackedCfa {
    frame: Extent,
    turns: u8,
    samples: HostSamples,
    mask: Option<HostSamples>,
}
impl PackedCfa {
    pub fn new(
        frame: Extent,
        turns: u8,
        samples: Vec<f32>,
        mask: Option<Vec<f32>>,
    ) -> EngineResult<Self> {
        Self::from_samples(
            frame,
            turns,
            HostSamples::Array(samples.into()),
            mask.map(|m| HostSamples::Array(m.into())),
        )
    }
    /// Retains ownership of runtime output without copying its sample buffer.
    /// This does not change the host-tensor runtime or imply GPU zero-copy.
    #[cfg(feature = "ml-denoise")]
    pub fn from_tensors(
        frame: Extent,
        turns: u8,
        samples: ml_runtime::Tensor,
        mask: Option<ml_runtime::Tensor>,
    ) -> EngineResult<Self> {
        let (w, h) = if turns.is_multiple_of(2) {
            (frame.width.div_ceil(2), frame.height.div_ceil(2))
        } else {
            (frame.height.div_ceil(2), frame.width.div_ceil(2))
        };
        let shape = [1, 4, h as usize, w as usize];
        if samples.shape() != shape || mask.as_ref().is_some_and(|m| m.shape() != shape) {
            return Err(EngineError::invalid("packed CFA", "tensor shape mismatch"));
        }
        Self::from_samples(
            frame,
            turns,
            HostSamples::Tensor(std::sync::Arc::new(samples)),
            mask.map(|m| HostSamples::Tensor(std::sync::Arc::new(m))),
        )
    }
    fn from_samples(
        frame: Extent,
        turns: u8,
        samples: HostSamples,
        mask: Option<HostSamples>,
    ) -> EngineResult<Self> {
        if frame.width == u32::MAX || frame.height == u32::MAX {
            return Err(EngineError::invalid("packed CFA", "padded extent overflow"));
        }
        let n = u64::from(frame.width.div_ceil(2)) * u64::from(frame.height.div_ceil(2)) * 4;
        if frame.width < 2
            || frame.height < 2
            || frame.width == u32::MAX
            || frame.height == u32::MAX
            || turns > 3
            || n != samples.len() as u64
            || samples.iter().any(|v| !v.is_finite())
            || mask.as_ref().is_some_and(|m| {
                m.len() != samples.len() || m.iter().any(|v| !(0.0..=1.0).contains(v))
            })
        {
            return Err(EngineError::invalid(
                "packed CFA",
                "invalid extent, rotation, samples or mask",
            ));
        }
        Ok(Self {
            frame,
            turns,
            samples,
            mask,
        })
    }
    pub fn frame(&self) -> Extent {
        self.frame
    }
    pub fn turns(&self) -> u8 {
        self.turns
    }
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    pub fn mask(&self) -> Option<&[f32]> {
        self.mask.as_deref()
    }
    pub fn bytes(&self) -> usize {
        self.samples.len() * 4 * (1 + usize::from(self.mask.is_some()))
    }
    pub fn packed_extent(&self) -> Extent {
        let e = Extent::new(self.frame.width.div_ceil(2), self.frame.height.div_ceil(2));
        if self.turns.is_multiple_of(2) {
            e
        } else {
            Extent::new(e.height, e.width)
        }
    }
    /// Sensor coordinate to rotated pixel (not packed-cell) coordinate.
    pub fn rotated_site(&self, x: u32, y: u32) -> (u32, u32) {
        let w = self.frame.width.div_ceil(2) * 2;
        let h = self.frame.height.div_ceil(2) * 2;
        match self.turns {
            0 => (x, y),
            1 => (y, w - 1 - x),
            2 => (w - 1 - x, h - 1 - y),
            _ => (h - 1 - y, x),
        }
    }
    pub fn blend_cpu(&self, input: &Image, amount: f32) -> EngineResult<Image> {
        if input.width() != self.frame.width
            || input.height() != self.frame.height
            || input.planes().len() != 1
            || !(0.0..=1.0).contains(&amount)
        {
            return Err(EngineError::invalid("CFA blend", "input extent or Amount"));
        }
        let e = self.packed_extent();
        let n = e.area() as usize;
        let mut result = Vec::with_capacity(self.frame.area() as usize);
        for y in 0..self.frame.height {
            for x in 0..self.frame.width {
                let (rx, ry) = self.rotated_site(x, y);
                let i = ((ry % 2) * 2 + rx % 2) as usize * n + (ry / 2 * e.width + rx / 2) as usize;
                let a = input.planes()[0][(y * self.frame.width + x) as usize];
                let b = self.samples[i];
                let alpha = amount * self.mask.as_ref().map_or(1.0, |m| m[i]);
                result.push(if alpha == 0.0 {
                    a
                } else if alpha == 1.0 {
                    b
                } else {
                    a * (1.0 - alpha) + b * alpha
                });
            }
        }
        Image::new(self.frame.width, self.frame.height, vec![result])
    }
}

pub(crate) type InferenceMemo = Option<(engine_api::stage::MemoKey, std::sync::Arc<PackedCfa>)>;
