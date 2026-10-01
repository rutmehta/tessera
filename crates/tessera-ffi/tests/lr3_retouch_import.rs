//! Synthetic catalog -> recipe -> existing CPU clone/heal operators.
use brush::{Brush, CloneSource, InputPoint, PaintMode, Stroke, Tip};
use compositor::{Depth, Raster, Rect};
use engine_api::{
    recipe::{
        MaskKind,
        mask::{RetouchKind, RetouchTarget},
    },
    tile::Extent,
};

#[test]
fn lr3_synthetic_catalog_clone_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let c = rusqlite::Connection::open(&fixture.catalog).unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=30", ["s = { RetouchInfo = {{ centerX=0.25, centerY=0.5, radius=0.0625, sourceX=0.75, sourceY=0.5, spotType='clone', opacity=0.5, feather=0.5 }} }"]).unwrap();
    drop(c);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    let recipe = &plan
        .images
        .iter()
        .find(|i| i.catalog_id == 30)
        .unwrap()
        .recipe;
    assert_eq!(recipe.settings.locals.retouch.len(), 1);
    assert!(recipe.unknown.contains_key("lrcat_develop_source"));
    let op = &recipe.settings.locals.retouch[0];
    let RetouchTarget::Area { components } = &op.target else {
        panic!()
    };
    let MaskKind::Brush { strokes } = &components[0].kind else {
        panic!()
    };
    let stroke = &strokes[0];
    let extent = Extent::new(128, 80);
    let mut image = Raster::new(extent, 3, Depth::F32, 0.0);
    image
        .edit_region(Rect::of_extent(extent), 1, |x, _, p| {
            *p = if x >= 64 {
                [1.0; 4]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
        })
        .unwrap();
    let source_offset = match op.kind {
        RetouchKind::Clone { source_offset } | RetouchKind::Heal { source_offset } => source_offset,
        _ => panic!(),
    };
    let source = CloneSource {
        offset: [source_offset[0] * 128.0, source_offset[1] * 80.0],
        source: None,
    };
    let brush = Brush {
        size: stroke.radius * 256.0,
        opacity: op.opacity / 100.0,
        flow: stroke.flow / 100.0,
        tip: Tip::round(1.0 - stroke.feather / 100.0),
        mode: PaintMode::Clone(source),
        ..Brush::default()
    };
    let mut render = Stroke::new(brush, &image, 1).unwrap();
    for p in &stroke.points {
        render
            .add_point(InputPoint::at(p[0] * 128.0, p[1] * 80.0).pressure(p[2]))
            .unwrap();
    }
    render.finish().unwrap();
    render
        .apply(&mut image, Rect::of_extent(extent), 2)
        .unwrap();
    let center = image.pixel(32, 40)[0];
    let shoulder = image.pixel(38, 40)[0];
    assert!((center - 0.5).abs() < 1e-5, "opacity: {center}");
    // Hand reference: 0.5*smoothstep((8.5-sqrt(6.5^2+0.5^2))/5).
    assert!((shoulder - 0.17323937).abs() < 1e-5, "feather: {shoulder}");
    assert_eq!(image.pixel(41, 40)[0], 0.0);
    let (mut mass, mut mx, mut my) = (0.0, 0.0, 0.0);
    for y in 0..80 {
        for x in 0..64 {
            let v = image.pixel(x, y)[0] as f64;
            mass += v;
            mx += v * (x as f64 + 0.5);
            my += v * (y as f64 + 0.5);
        }
    }
    let error = ((mx / mass - 32.0).powi(2) + (my / mass - 40.0).powi(2)).sqrt();
    assert!(error <= 1.0, "position error {error}");
    println!(
        "LR-3 synthetic 128x80: centroid_error_px={error:.8}, center={center:.8}, feather_shoulder={shoulder:.8}"
    );
}

#[test]
fn synthetic_catalog_to_develop_cpu() {
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let c = rusqlite::Connection::open(&fixture.catalog).unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=30", ["s = { RetouchInfo = {{ centerX=0.25, centerY=0.5, radius=0.0625, sourceX=0.75, sourceY=0.5, spotType='clone', opacity=0.5, feather=0.5 }} }"]).unwrap();
    drop(c);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    let recipe = &plan
        .images
        .iter()
        .find(|i| i.catalog_id == 30)
        .unwrap()
        .recipe;
    assert_eq!(recipe.settings.locals.retouch.len(), 1);
    let plane = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 0.8 } else { 0.1 }))
        .collect();
    let pixels = pipeline_cpu::Image::new(128, 80, vec![plane; 3]).unwrap();
    let raw = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(44),
        image_core::RgbSource::from_linear_rec2020(pixels).unwrap(),
    )
    .unwrap();
    let renderer = image_core::Renderer::new(Default::default())
        .with_retouch_renderer(Arc::new(brush::render_retouch))
        .for_recipe(recipe);
    let rect = image_core::PixelRect::full(raw.active_extent());
    let mut baseline = recipe.settings.clone();
    baseline.locals.retouch.clear();
    let before = renderer
        .render_region_as(
            &raw,
            &baseline,
            0,
            rect,
            image_core::RenderOutput::SceneLinear,
        )
        .unwrap();
    let after = renderer
        .render_region_as(
            &raw,
            &recipe.settings,
            0,
            rect,
            image_core::RenderOutput::SceneLinear,
        )
        .unwrap();
    let l = after[0].layout();
    let i = l.index(0, 32, 40).unwrap();
    assert!(after[0].samples::<f32>().unwrap()[i] > before[0].samples::<f32>().unwrap()[i] + 0.1);
}
