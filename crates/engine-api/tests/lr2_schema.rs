use engine_api::recipe::{
    required_schema_version,
    settings::{LegacyPv2010, MonochromeSettings, ToneCurves},
    DevelopSettings, Recipe,
};

#[test]
fn lr2c_schema_predicate_only_requires_v4_for_used_lane_fields() {
    let mut r = Recipe::default();
    assert_eq!(required_schema_version(&r), 3);
    r.settings.tone.exposure = 1.;
    assert_eq!(required_schema_version(&r), 3);
    r.settings.color.monochrome = Some(MonochromeSettings::default());
    assert_eq!(required_schema_version(&r), 3);
    r.settings.color.monochrome.as_mut().unwrap().enabled = true;
    assert_eq!(required_schema_version(&r), 4);
    r.settings.color.monochrome = None;
    r.settings.tone.legacy_pv2010 = Some(LegacyPv2010::default());
    assert_eq!(required_schema_version(&r), 4);
    r.settings.tone.legacy_pv2010 = None;
    r.settings.tone.curves_extended = Some(ToneCurves::default());
    assert_eq!(required_schema_version(&r), 4);
    r.settings.tone.curves_extended = None;
    assert_eq!(required_schema_version(&r), 3);
}

#[test]
fn lr2c_bw_mixer_invalidates_tone_cache_only_when_enabled() {
    let mut s = DevelopSettings::default();
    let base = s.stage_hashes();
    s.color.monochrome = Some(MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    let enabled = s.stage_hashes();
    assert_ne!(
        base[engine_api::stage::StageId::Tone.index()],
        enabled[engine_api::stage::StageId::Tone.index()]
    );
    s.color.monochrome.as_mut().unwrap().mixer.red = 30.;
    assert_ne!(
        enabled[engine_api::stage::StageId::Tone.index()],
        s.stage_hashes()[engine_api::stage::StageId::Tone.index()]
    );
    s.color.monochrome.as_mut().unwrap().enabled = false;
    assert_eq!(
        base[engine_api::stage::StageId::Tone.index()],
        s.stage_hashes()[engine_api::stage::StageId::Tone.index()]
    );
}
