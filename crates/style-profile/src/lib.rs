//! Library-local, non-generative base-edit learning.
pub mod features;
pub use features::Features;
mod batch;
mod library;
mod perception;
pub use perception::PerceptionInput;
mod model;
mod sliders;
pub use batch::{
    apply_batch, group_amount, BatchImage, BatchJob, GroupAmount, RecipeStore, ReviewEntry,
};
pub use library::SidecarStore;
pub use model::{Prediction, Profile, Questionnaire, ReferenceEdit, SliderPrediction};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn questionnaire_directions() {
        let p = Profile::new(
            "library",
            Questionnaire {
                brightness: 1.,
                contrast: -1.,
                warmth: 1.,
                saturation: -1.,
                skin_tone_priority: 1.,
            },
        )
        .unwrap();
        let f = Features {
            face_count: 1,
            face_mean_luminance: Some(0.07),
            ..Features::default()
        };
        let result = p.predict(&f).unwrap();
        assert!(result.settings.tone.exposure > 0.);
        assert!(result.settings.tone.contrast < 0.);
        assert!(result.settings.white_balance.temperature > 5500.);
        assert!(result.settings.color.saturation < 0.);
        assert!(result.settings.tone.shadows > 0.);
        assert!(result.sliders.iter().all(|s| s.confidence <= 0.2));
    }
}
