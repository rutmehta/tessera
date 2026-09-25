use crate::{CullSession, ImageId};
use engine_api::{EngineError, EngineResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Below,
    Above,
}
/// Signal names are open-ended, including future per-face signals. Values use
/// the producer's scale. Equality is not a defect. Absent signals are ignored.
#[derive(Debug, Clone)]
pub struct Threshold {
    pub signal: String,
    pub value: f64,
    pub direction: Direction,
}
impl Threshold {
    pub fn below(signal: impl Into<String>, value: f64) -> Self {
        Self {
            signal: signal.into(),
            value,
            direction: Direction::Below,
        }
    }
    pub fn above(signal: impl Into<String>, value: f64) -> Self {
        Self {
            signal: signal.into(),
            value,
            direction: Direction::Above,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct DefectReason {
    pub signal: String,
    pub value: f64,
    pub threshold: f64,
    pub direction: Direction,
    pub model: String,
}
impl CullSession<'_> {
    /// Read-only review list. Never applies decisions or adds undo entries.
    pub fn defect_sweep(
        &self,
        thresholds: &[Threshold],
    ) -> EngineResult<Vec<(ImageId, Vec<DefectReason>)>> {
        if thresholds
            .iter()
            .any(|t| !t.value.is_finite() || t.signal.is_empty())
        {
            return Err(EngineError::invalid(
                "threshold",
                "requires signal and finite value",
            ));
        }
        let mut out = Vec::new();
        for id in &self.images {
            let scores = self.index.scores(*id)?;
            let mut reasons = Vec::new();
            for threshold in thresholds {
                for score in scores.iter().filter(|s| s.signal == threshold.signal) {
                    let defective = match threshold.direction {
                        Direction::Below => score.value < threshold.value,
                        Direction::Above => score.value > threshold.value,
                    };
                    if defective {
                        reasons.push(DefectReason {
                            signal: score.signal.clone(),
                            value: score.value,
                            threshold: threshold.value,
                            direction: threshold.direction,
                            model: score.model.clone(),
                        });
                    }
                }
            }
            if !reasons.is_empty() {
                out.push((*id, reasons));
            }
        }
        Ok(out)
    }
}
