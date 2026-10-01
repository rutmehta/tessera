// Synthetic SQLite catalog -> Lua develop -> recipe -> CPU operators.
use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let f = import_lrcat::fixture::write(dir.path()).unwrap();
    let c = rusqlite::Connection::open(&f.catalog).unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='5.7' WHERE image=30", ["s = { Exposure=1, ConvertToGrayscale=true, GrayMixerRed=50, GrayMixerOrange=50, GrayMixerYellow=50, GrayMixerGreen=50, GrayMixerAqua=50, GrayMixerBlue=50, GrayMixerPurple=50, GrayMixerMagenta=50, ExtendedToneCurvePV2012={0,0,255,255} }"]).unwrap();
    drop(c);
    let plan = import_lrcat::import(&f.catalog).unwrap();
    let r = &plan
        .images
        .iter()
        .find(|i| i.catalog_id == 30)
        .unwrap()
        .recipe;
    r.validate().unwrap();
    for key in [
        "ConvertToGrayscale",
        "GrayMixerRed",
        "ExtendedToneCurvePV2012",
    ] {
        assert!(
            r.unknown
                .get("lrcat_develop_source")
                .is_none_or(|source| source["properties"][key].is_null())
        );
    }
    let mut max_error = 0f32;
    for (rgb, expected) in [
        ([1., 0., 0.], 0.7881),
        ([0., 1., 0.], 2.034),
        ([0., 0., 1.], 0.1779),
        ([0.18; 3], 0.36),
    ] {
        let mut tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 3,
            },
            rgb.to_vec(),
        )
        .unwrap();
        pipeline_cpu::tone(&mut tile, &r.settings.tone).unwrap();
        pipeline_cpu::tone_extra(&mut tile, &r.settings.tone).unwrap();
        pipeline_cpu::color(&mut tile, &r.settings.color).unwrap();
        for value in tile.samples::<f32>().unwrap() {
            max_error = max_error.max((value - expected).abs());
        }
    }
    println!(
        "synthetic import: 4 swatches / 12 channels, maximum absolute error={max_error:.9}, tolerance=0.000002"
    );
    assert!(max_error < 2e-6);

    // Analytic reference at the interior control point in Tessera's log domain:
    // linear = 0.18 * ((1 + 1/0.18) ** normalized_coordinate - 1).
    let input = 0.2825710525f32;
    let expected = 0.4056726422f32;
    let mut curve_error = 0f32;
    for (suffix, channel) in [
        ("", None),
        ("Red", Some(0)),
        ("Green", Some(1)),
        ("Blue", Some(2)),
    ] {
        let row = format!("s = {{ ExtendedToneCurvePV2012{suffix}={{0,0,128,160,255,255}} }}");
        let c = rusqlite::Connection::open(&f.catalog).unwrap();
        c.execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=30",
            [&row],
        )
        .unwrap();
        drop(c);
        let plan = import_lrcat::import(&f.catalog).unwrap();
        let r = &plan
            .images
            .iter()
            .find(|i| i.catalog_id == 30)
            .unwrap()
            .recipe;
        let mut tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 3,
            },
            vec![input; 3],
        )
        .unwrap();
        pipeline_cpu::tone_extra(&mut tile, &r.settings.tone).unwrap();
        for (i, v) in tile.samples::<f32>().unwrap().iter().enumerate() {
            let target = if channel.is_none() || channel == Some(i) {
                expected
            } else {
                input
            };
            curve_error = curve_error.max((v - target).abs());
        }
    }
    println!(
        "synthetic extended curves: 4 catalog imports / 12 channels, maximum absolute error={curve_error:.9}, tolerance=0.000002"
    );
    assert!(curve_error < 2e-6);

    // LR-2b: both optional blocks travel through a synthetic SQLite import,
    // history round-trip and the CPU operators. No real catalog or RAW input.
    for (row, input, expected) in [
        (
            "s={Exposure=1,Brightness=100,AutoToneDigest='cache'}",
            0.125,
            0.4,
        ),
        (
            "s={ExtendedToneCurvePV2012={0,0,510,765}}",
            7.5555556,
            50.53086419753086,
        ),
    ] {
        let c = rusqlite::Connection::open(&f.catalog).unwrap();
        c.execute(
            "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='5.7' WHERE image=30",
            [row],
        )
        .unwrap();
        drop(c);
        let plan = import_lrcat::import(&f.catalog).unwrap();
        let r = &plan
            .images
            .iter()
            .find(|i| i.catalog_id == 30)
            .unwrap()
            .recipe;
        let back = engine_api::recipe::Recipe::from_json(&r.to_json().unwrap()).unwrap();
        assert_eq!(back.settings, r.settings);
        let mut t = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 3,
            },
            vec![input; 3],
        )
        .unwrap();
        pipeline_cpu::tone(&mut t, &r.settings.tone).unwrap();
        pipeline_cpu::tone_extra(&mut t, &r.settings.tone).unwrap();
        let error = (t.samples::<f32>().unwrap()[0] - expected).abs();
        assert!(error < 0.0001, "{row}: {error}");
        assert!(!plan.report.iter().any(|v| v.contains("AutoToneDigest")));
        println!("LR-2b synthetic field import/render: {row}; max absolute error={error:.9}");
    }
}
