//! Forty-four invented legacy groups, deliberately excluding new LR-4 shapes.
pub fn legacy_groups() -> String {
    let groups: Vec<_> = (0..44).map(|i| {
        let mask = if i % 2 == 0 {
            format!(r#"{{ What = "Mask/Gradient", FullX = 0.1, FullY = 0.2, ZeroX = 0.8, ZeroY = 0.9, MaskActive = true, MaskBlendMode = {}, MaskInverted = {} }}"#, i % 3, i % 4 == 0)
        } else {
            format!(r#"{{ What = "Mask/CircularGradient", Left = 0.2, Right = 0.8, Top = 0.1, Bottom = 0.9, Feather = 30, Angle = {}, MaskActive = true }}"#, i)
        };
        format!(r#"{{ What = "Correction", CorrectionName = "Synthetic group {i}", CorrectionAmount = 1, LocalExposure2012 = {}, CorrectionMasks = {{ {mask} }} }}"#, i as f32 / 100.)
    }).collect();
    format!(
        r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{ {} }} }}"#,
        groups.join(",")
    )
}
pub fn digest() -> (String, usize) {
    let (r, _) = import_lrcat::lua_develop::parse(&legacy_groups(), "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 44);
    let bytes = serde_json::to_vec(&r).unwrap();
    (
        engine_api::id::Digest::derive("LR-4 legacy recipe", &bytes).to_string(),
        bytes.len(),
    )
}
