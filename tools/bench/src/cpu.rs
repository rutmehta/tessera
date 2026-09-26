//! Fixed, versioned synthetic fixtures. Setup is outside timed regions.
use anyhow::{Result, ensure};
use engine_api::{
    recipe::DevelopSettings,
    tile::{Extent, Tile, TileCoord},
};
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn sample(
    rows: &mut Vec<Value>,
    bench: &str,
    fixture: &str,
    mut run: impl FnMut() -> Result<()>,
) -> Result<()> {
    run()?; // One untimed warmup, then five independent operations.
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        run()?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    rows.push(
        json!({"bench": bench, "fixture": fixture, "metric": "median_ms",
        "value": times[2], "unit": "ms", "backend": "cpu",
        "host": std::env::var("BENCH_HOST").unwrap_or_else(|_| "local".into())}),
    );
    Ok(())
}

pub fn measure() -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("index.db");
    let index = index::Index::open(&db)?;
    let mut conn = rusqlite::Connection::open(&db)?;
    // Mirror the existing ignored index benchmark, through the migrated schema.
    conn.execute_batch(
        "INSERT INTO root(id,path) VALUES(1,'/bench');
        INSERT INTO folder(id,root_id,path) VALUES(1,1,'/bench/images');",
    )?;
    let tx = conn.transaction()?;
    {
        let mut file = tx.prepare("INSERT INTO file(id,folder_id,path,name,size,mtime) VALUES(?1,1,?2,?2,24000000,1780000000)")?;
        let mut image = tx.prepare("INSERT INTO image(id,file_id,capture_time,camera,lens,caption) VALUES(?1,?2,'2026-06-01','Camera 0','Lens 0',?3)")?;
        let mut fts = tx.prepare("INSERT INTO fts(rowid,image_id,filename,keywords,caption,camera,lens) VALUES((SELECT rowid FROM image WHERE id=?1),?1,?2,'travel',?3,'Camera 0','Lens 0')")?;
        for n in 1..=100_000_u32 {
            let id = engine_api::id::ImageId(n.into()).to_string();
            let name = format!("IMG_{n:06}.jpg");
            let caption = if n % 10 == 0 {
                "travel landscape mountain"
            } else {
                "travel portrait city"
            };
            file.execute(rusqlite::params![n, name])?;
            image.execute(rusqlite::params![id, n, caption])?;
            fts.execute(rusqlite::params![id, name, caption])?;
        }
    }
    tx.commit()?;
    drop(conn);
    let query = index::Query {
        text: Some("landscape".into()),
        limit: 100,
        ..Default::default()
    };
    sample(&mut rows, "index-search", "100k-fts-v1", || {
        ensure!(black_box(index.search(&query)?).len() == 100);
        Ok(())
    })?;

    let path = sidecar::Sidecar::paths(dir.path().join("image.raw"));
    let document = sidecar::RecipeDocument::default();
    let packet =
        sidecar::XmpPacket::from_selection(&Default::default(), &sidecar::MarkPreset::lightroom());
    sample(
        &mut rows,
        "sidecar-roundtrip",
        "recipe-xmp-fsync-v1",
        || {
            sidecar::Sidecar::write_recipe(&path.recipe, &document)?;
            sidecar::Sidecar::write_xmp(&path.xmp, &packet)?;
            ensure!(sidecar::Sidecar::read_recipe(&path.recipe)? == document);
            ensure!(sidecar::Sidecar::read_xmp(&path.xmp)?.serialize() == packet.serialize());
            Ok(())
        },
    )?;

    let preview = image::RgbImage::from_fn(8000, 5625, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
    });
    let store = previews::PreviewStore::new(dir.path().join("previews"), u64::MAX)?;
    let mut serial = 0_u64;
    sample(&mut rows, "preview-pyramid", "45mp-gradient-v1", || {
        serial += 1;
        let key = previews::PreviewKey::new(&serial.to_le_bytes(), 1, [0; 32]);
        store.ensure(&key, previews::Level::Eighth, &|_| preview.clone())?;
        ensure!(store.get(&key, previews::Level::Eighth).is_some());
        Ok(())
    })?;
    drop(preview);

    let (w, h) = (2048_u32, 1536_u32);
    let planes = (0..3)
        .map(|c| {
            (0..w * h)
                .map(|i| 0.01 + ((i % w + (i / w) * 3 + c * 71) % 1024) as f32 / 1100.0)
                .collect()
        })
        .collect();
    let image = pipeline_cpu::Image::new(w, h, planes)?;
    let source = pipeline_cpu::RenderSource::Rgb(&image);
    sample(
        &mut rows,
        "pipeline-cpu-l3",
        "rgb-2048x1536-scale8-v1",
        || {
            let output = pipeline_cpu::render_scaled(&DevelopSettings::default(), &source, 8)?;
            ensure!(output.dimensions() == (256, 192));
            black_box(output);
            Ok(())
        },
    )?;

    let recipe = engine_api::recipe::Recipe::default();
    let cancel = Default::default();
    let settings = export::ExportSettings {
        output_dir: dir.path().join("exports"),
        format: export::Format::Jpeg { quality: 85 },
        resize: export::Resize::LongEdge(2048),
        sharpen_for: export::SharpenFor::Screen,
        render_scale: 1,
        apply_orientation: true,
        ..Default::default()
    };
    let mut sequence = 0;
    sample(&mut rows, "export-web", "rgb-2048x1536-web-q85-v1", || {
        sequence += 1;
        let name = format!("bench-{sequence}");
        let item = export::ExportImage {
            source: pipeline_cpu::RenderSource::Rgb(&image),
            name: &name,
            sequence,
            date: "",
            metadata: None,
        };
        let rendered =
            export::render_one_cancellable(&item, &recipe, &settings, &cancel, None, None)?;
        ensure!(!rendered.used_gpu(), "CPU benchmark unexpectedly used GPU");
        let path = rendered.finish(&cancel)?;
        ensure!(std::fs::metadata(path)?.len() > 100);
        Ok(())
    })?;

    use compositor::*;
    let extent = Extent::new(1024, 768);
    let mut doc = Document::new(DocState::new(extent, Depth::U8));
    for seed in 0..20_u32 {
        let mut layer = Layer::pixel(format!("layer {seed}"), extent, Depth::U8);
        layer.props.opacity = 0.8;
        let raster = layer.raster_mut().unwrap();
        let (cols, rows) = raster.grid();
        for ty in 0..rows {
            for tx in 0..cols {
                let layout = raster.layout(tx, ty);
                let n = layout.plane_len();
                let mut pixels = vec![0_u8; n * 4];
                for i in 0..n {
                    pixels[i] = ((i as u32 + seed * 17) % 256) as u8;
                    pixels[n + i] = ((i as u32 / 7 + seed * 31) % 256) as u8;
                    pixels[2 * n + i] = ((i as u32 / 11 + seed * 13) % 256) as u8;
                    pixels[3 * n + i] = 180;
                }
                let tile = Tile::from_samples(TileCoord::new(0, tx, ty), layout, pixels)?;
                raster.set_slot(tx, ty, Some(tile), 1)?;
            }
        }
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer,
        })?;
    }
    let compositor = Compositor::new(256 << 20);
    sample(
        &mut rows,
        "compositor-20-layer",
        "rgba8-1024x768-l0-v1",
        || {
            compositor.clear_composites();
            let tiles = compositor.render_level(&doc, 0, &cancel)?;
            ensure!(!tiles.is_empty());
            black_box(tiles);
            Ok(())
        },
    )?;
    Ok(rows)
}
