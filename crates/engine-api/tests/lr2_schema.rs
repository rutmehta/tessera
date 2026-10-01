use engine_api::recipe::{
    settings::{LegacyPv2010, MonochromeSettings, ToneCurves},
    DevelopSettings,
};

#[test]
fn lr2c_schema_predicate_only_requires_v4_for_used_lane_fields() {
    let mut s = DevelopSettings::default();
    assert_eq!(s.required_schema_version_lr2(), 3);
    s.tone.exposure = 1.;
    assert_eq!(s.required_schema_version_lr2(), 3);
    s.color.monochrome = Some(MonochromeSettings::default());
    assert_eq!(s.required_schema_version_lr2(), 3);
    s.color.monochrome.as_mut().unwrap().enabled = true;
    assert_eq!(s.required_schema_version_lr2(), 4);
    s.color.monochrome = None;
    s.tone.legacy_pv2010 = Some(LegacyPv2010::default());
    assert_eq!(s.required_schema_version_lr2(), 4);
    s.tone.legacy_pv2010 = None;
    s.tone.curves_extended = Some(ToneCurves::default());
    assert_eq!(s.required_schema_version_lr2(), 4);
    s.tone.curves_extended = None;
    assert_eq!(s.required_schema_version_lr2(), 3);
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
