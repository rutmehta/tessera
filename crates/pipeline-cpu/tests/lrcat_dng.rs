#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

#[test]
fn camera_dng_refuses_unconsumed_opcodes_instead_of_dropping_corrections() {
    let mut dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    // Valid optional unknown opcode. It may be ignored by a generic parser,
    // but this cropped source has not executed its pixel-correction lists.
    dng.metadata.opcode_lists[1] = Some(
        [1u32, 999, 0x01030000, 1, 0]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect(),
    );
    assert!(pipeline_cpu::CameraLinearProxy::from_dng(dng).is_err());
}

/// Private diagnostics use only numeric output. No source identifiers or errors escape.
#[test]
#[ignore = "requires TESSERA_LR8_RENDER_INPUT and TESSERA_LR8_RENDER_OUTPUT"]
fn default_render_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let run = || -> Option<()> {
        let input = std::env::var_os("TESSERA_LR8_RENDER_INPUT")?;
        let output = std::env::var_os("TESSERA_LR8_RENDER_OUTPUT")?;
        let input = std::path::Path::new(&input);
        let settings = engine_api::recipe::DevelopSettings::default();
        let mut file = std::fs::File::open(input).ok()?;
        let proxy = if input.extension().is_some_and(|e| e.eq_ignore_ascii_case("dng")) {
            raw_decode::lossy_dng::read(&mut file).ok()?
        } else { None };
        let pixels = if let Some(dng) = proxy {
            println!("BaselineExposure={}", dng.baseline_exposure);
            println!("AsShotWb={:?}", dng.metadata.as_shot_wb);
            println!("ColorMatrix={:?}", dng.metadata.cam_xyz);
            println!("OpcodeListBytes={:?}", dng.metadata.opcode_lists.each_ref().map(|v|v.as_ref().map_or(0,Vec::len)));
            let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng).map_err(|_|println!("FailureStage=1")).ok()?;
            pipeline_cpu::render_scaled(&settings, &pipeline_cpu::RenderSource::CameraLinear(&proxy), 4).map_err(|_|println!("FailureStage=2")).ok()?
        } else {
            let mut raw = raw_decode::RawSource::open(input).ok()?;
            let cfa = raw.decode_cfa().ok()?;
            let metadata = raw.metadata();
            println!("AsShotWb={:?}", metadata.as_shot_wb);
            println!("ColorMatrix={:?}", metadata.cam_xyz);
            pipeline_cpu::render_scaled(&settings, &pipeline_cpu::RenderSource::Cfa { image: &cfa, metadata: &metadata }, 8).ok()?
        };
        let file = std::fs::File::create(output).ok()?;
        let mut encoder = png::Encoder::new(file,pixels.width(),pixels.height());
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder.write_header().ok()?.write_image_data(pixels.as_raw()).ok()?;
        let mean: [f64;3] = std::array::from_fn(|c| pixels.pixels().map(|p| f64::from(p[c])).sum::<f64>() / f64::from(pixels.width()*pixels.height()));
        println!("MeanRgb={mean:?}");
        Some(())
    };
    assert!(run().is_some(), "numeric diagnostic failed");
}
