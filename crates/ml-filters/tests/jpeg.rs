use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use ml_filters::{Cancel, JpegArtifactRemoval, NeuralFilter, Params, estimate_jpeg_quality};
use ml_runtime::{ModelRegistry, SessionOptions};

#[test]
fn boundary_estimate_and_zero_bypass() -> anyhow::Result<()> {
    let mut r = Raster::new(Extent::new(32, 32), 4, Depth::F32, 0.5);
    let clean = estimate_jpeg_quality(&r, &Cancel::new())?;
    r.edit_region(Rect::new(0, 0, 32, 32), 1, |x, _, p| {
        p[..3].fill(0.4 + (x / 8 % 2) as f32 * 0.1);
    })?;
    assert!(estimate_jpeg_quality(&r, &Cancel::new())? < clean);
    let f = JpegArtifactRemoval::unloaded();
    let zero = Params {
        strength: 0.0,
        ..Params::default()
    };
    assert!(
        f.apply(&r, &zero, &Cancel::new())?
            .shares_all_tiles_with(&r)
    );
    assert!(f.apply(&r, &Params::default(), &Cancel::new()).is_err());
    Ok(())
}

#[test]
fn cached_q30_jpeg_improves_psnr() -> anyhow::Result<()> {
    let Some(cache) = std::env::var_os("TESSERA_FILTER_MODEL_CACHE") else {
        eprintln!("SKIP: set TESSERA_FILTER_MODEL_CACHE for real DRUNet PSNR");
        return Ok(());
    };
    let registry = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        cache,
    )?;
    let reference = engine_api::id::ModelRef {
        id: ml_enhance::DENOISE_MODEL_ID.into(),
        version: ml_enhance::DENOISE_VERSION.into(),
    };
    if registry.resolve_cached_ref(&reference)?.is_none() {
        eprintln!("SKIP: DRUNet absent");
        return Ok(());
    }
    let clean = image::RgbImage::from_fn(64, 64, |x, y| {
        let v = (80.0 + 70.0 * x as f32 / 63.0 + 20.0 * (y as f32 / 13.0).sin()) as u8;
        image::Rgb([v, v.saturating_add(12), v.saturating_sub(8)])
    });
    let mut bytes = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 30).encode_image(&clean)?;
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    let mut input = Raster::new(Extent::new(64, 64), 4, Depth::F32, 0.0);
    input.edit_region(Rect::new(0, 0, 64, 64), 1, |x, y, p| {
        let rgb = decoded.get_pixel(x, y).0;
        *p = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            0.8,
        ];
    })?;
    let filter = JpegArtifactRemoval::load(&registry, SessionOptions::cpu())?;
    let result = filter.apply(
        &input,
        &Params {
            strength: 1.0,
            ..Params::default()
        },
        &Cancel::new(),
    )?;
    let mse = |r: &Raster| -> f64 {
        let mut sum = 0.0;
        for (x, y, px) in clean.enumerate_pixels() {
            for c in 0..3 {
                sum += (r.pixel(x, y)[c] as f64 - px[c] as f64 / 255.0).powi(2);
            }
        }
        sum / (64.0 * 64.0 * 3.0)
    };
    let before = mse(&input);
    let after = mse(&result);
    println!(
        "q30 JPEG PSNR: {:.3} -> {:.3} dB",
        -10.0 * before.log10(),
        -10.0 * after.log10()
    );
    assert!(after < before, "DRUNet did not improve q30 PSNR");
    assert_eq!(result.pixel(10, 10)[3], 0.8);
    Ok(())
}
