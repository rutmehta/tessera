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

#[test]
fn baseline_exposure_is_shared_by_cfa_and_external_camera_linear_sources() {
    let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false,false))).unwrap().unwrap();
    dng.width = 32; dng.height = 24;
    dng.metadata.width = 32; dng.metadata.height = 24;
    dng.metadata.default_crop = [0,0,32,24];
    dng.metadata.orientation = 1;
    dng.pixels = vec![[0.08;3];32*24];
    dng.baseline_exposure = 0.75;
    let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap();
    assert_eq!(proxy.original_metadata().baseline_exposure,0.75);
    // Exposure is a shared camera-profile operation, never baked into stored samples.
    assert_eq!(proxy.pixels().planes()[0][0],0.08);
    let mut metadata = proxy.original_metadata().clone();
    metadata.cfa_layout = raw_decode::CfaLayout::Bayer([[0,1],[1,2]]);
    let cfa = raw_decode::CfaImage::from_linear(32,24,vec![0.08;32*24]).unwrap();
    let mut s = engine_api::recipe::DevelopSettings::default();
    s.detail.sharpening.amount=0.; s.detail.noise_reduction.color=0.;
    let a = pipeline_cpu::render_linear_scaled(&s,&pipeline_cpu::RenderSource::Cfa{image:&cfa,metadata:&metadata},1).unwrap();
    let b = pipeline_cpu::render_linear_scaled(&s,&pipeline_cpu::RenderSource::CameraLinear(&proxy),1).unwrap();
    metadata.baseline_exposure=0.;
    let zero = pipeline_cpu::render_linear_scaled(&s,&pipeline_cpu::RenderSource::Cfa{image:&cfa,metadata:&metadata},1).unwrap();
    for ((a,b),zero) in a.planes().iter().flatten().zip(b.planes().iter().flatten()).zip(zero.planes().iter().flatten()) {
        assert!((a-b).abs()<1e-5);
        assert!((a-zero*2f32.powf(0.75)).abs()<1e-5);
    }
}
