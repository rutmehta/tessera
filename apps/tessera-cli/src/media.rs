//! File-oriented CLI rendering and camera-preview export.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
use engine_api::{
    id::ImageId,
    recipe::DevelopSettings,
    tile::{Extent, TILE_SIZE, Tile},
};
use image::{RgbImage, imageops};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, render::MAX_LEVEL};
use previews::Codec;
use raw_decode::RawSource;

/// Render the active area at pyramid `level` (0 = full size), apply camera
/// orientation, and save display-encoded RGB as JPEG or PNG. Settings are
/// supplied by the caller, including any recipe/CLI overrides already merged.
pub fn render(path: &Path, out: &Path, level: u8, settings: &DevelopSettings) -> Result<()> {
    ensure!(
        level <= MAX_LEVEL,
        "level must be between 0 and {MAX_LEVEL}"
    );
    let source = RawImage::open(ImageId::default(), path)
        .with_context(|| format!("decode RAW {}", path.display()))?;
    let pixels = render_image(&source, level, settings)?;
    save(&pixels, out)
}

/// Export a camera JPEG preview, bounded by `max` pixels on its longest edge.
/// Missing or undecodable embedded JPEGs fall back to the default RAW pipeline.
/// Small previews are not upscaled; `max` must be positive.
pub fn preview(path: &Path, out: &Path, max: u32) -> Result<()> {
    ensure!(max > 0, "preview max must be greater than zero");
    let mut source =
        RawSource::open(path).with_context(|| format!("open RAW {}", path.display()))?;
    let orientation = source.metadata().orientation;
    if let Some(pixels) = source
        .embedded_preview()
        .and_then(|bytes| decode_embedded(&bytes, orientation, max))
    {
        return save(&pixels, out);
    }

    let cfa = source
        .decode_cfa()
        .with_context(|| format!("decode RAW preview fallback {}", path.display()))?;
    // LibRaw may refine metadata during unpack; read it again afterwards.
    let raw = RawImage::new(
        ImageId::default(),
        Arc::new(cfa),
        Arc::new(source.metadata()),
    )?;
    let mut level = 0;
    while level < MAX_LEVEL {
        let next = raw.level_extent(level + 1);
        if next.width.max(next.height) < max {
            break;
        }
        level += 1;
    }
    let pixels = fit(render_image(&raw, level, &DevelopSettings::default())?, max);
    save(&pixels, out)
}

fn render_image(source: &RawImage, level: u8, settings: &DevelopSettings) -> Result<RgbImage> {
    let extent = source.level_extent(level);
    // This renderer/cache is private to this source, so a fresh default ID is safe.
    let renderer = Renderer::new(RendererConfig::default());
    let tiles = renderer
        .render_region(source, settings, level, PixelRect::full(extent))
        .context("render display tiles")?;
    Ok(orient(
        stitch(extent, &tiles)?,
        source.metadata().orientation,
    ))
}

fn stitch(extent: Extent, tiles: &[Tile]) -> Result<RgbImage> {
    ensure!(extent.width > 0 && extent.height > 0, "empty output extent");
    let mut output = RgbImage::new(extent.width, extent.height);
    for tile in tiles {
        let layout = tile.layout();
        ensure!(
            layout.channels == 3,
            "display tile must have three RGB planes"
        );
        let x0 = u64::from(tile.coord().x) * u64::from(TILE_SIZE);
        let y0 = u64::from(tile.coord().y) * u64::from(TILE_SIZE);
        ensure!(
            x0 + u64::from(layout.extent.width) <= u64::from(extent.width)
                && y0 + u64::from(layout.extent.height) <= u64::from(extent.height),
            "display tile lies outside output extent"
        );
        let planes = [
            tile.plane::<u8>(0)?,
            tile.plane::<u8>(1)?,
            tile.plane::<u8>(2)?,
        ];
        for y in 0..layout.extent.height {
            for x in 0..layout.extent.width {
                let index = (y as usize + layout.halo as usize) * layout.stride()
                    + x as usize
                    + layout.halo as usize;
                output.put_pixel(
                    x0 as u32 + x,
                    y0 as u32 + y,
                    image::Rgb([planes[0][index], planes[1][index], planes[2][index]]),
                );
            }
        }
    }
    Ok(output)
}

fn decode_embedded(bytes: &[u8], orientation: u16, max: u32) -> Option<RgbImage> {
    previews::Jpeg
        .decode(bytes)
        .ok()
        .map(|pixels| fit(orient(pixels, orientation), max))
}

fn orient(pixels: RgbImage, orientation: u16) -> RgbImage {
    match orientation {
        2 => imageops::flip_horizontal(&pixels),
        3 => imageops::rotate180(&pixels),
        4 => imageops::flip_vertical(&pixels),
        // EXIF 5 is transpose; EXIF 7 is transverse (not vice versa).
        5 => imageops::rotate90(&imageops::flip_vertical(&pixels)),
        6 => imageops::rotate90(&pixels),
        7 => imageops::rotate90(&imageops::flip_horizontal(&pixels)),
        8 => imageops::rotate270(&pixels),
        _ => pixels,
    }
}

fn fit(pixels: RgbImage, max: u32) -> RgbImage {
    let longest = pixels.width().max(pixels.height());
    if longest <= max {
        return pixels;
    }
    let scaled =
        |dimension: u32| (u64::from(dimension) * u64::from(max) / u64::from(longest)).max(1) as u32;
    imageops::resize(
        &pixels,
        scaled(pixels.width()),
        scaled(pixels.height()),
        imageops::FilterType::Lanczos3,
    )
}

fn save(pixels: &RgbImage, out: &Path) -> Result<()> {
    match image::ImageFormat::from_path(out).context("output needs a JPEG or PNG extension")? {
        image::ImageFormat::Jpeg => {
            let bytes = previews::Jpeg.encode(pixels).context("encode JPEG")?;
            std::fs::write(out, bytes).with_context(|| format!("write {}", out.display()))?;
        }
        image::ImageFormat::Png => pixels
            .save_with_format(out, image::ImageFormat::Png)
            .with_context(|| format!("write {}", out.display()))?,
        _ => anyhow::bail!("output must be JPEG or PNG: {}", out.display()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::tile::{TileCoord, TileLayout};

    #[test]
    fn stitches_planar_edge_tiles_and_ignores_halo() {
        let extent = Extent::new(TILE_SIZE + 1, 2);
        let tiles: Vec<_> = (0..2)
            .map(|column| {
                let layout = TileLayout {
                    extent: Extent::new(if column == 0 { TILE_SIZE } else { 1 }, 2),
                    halo: 1,
                    channels: 3,
                };
                let mut samples = vec![255u8; layout.len()];
                for y in 0..2 {
                    for x in 0..layout.extent.width {
                        for channel in 0..3 {
                            samples[layout.index(channel, x as i32, y).unwrap()] =
                                10 * channel + column as u8 + y as u8;
                        }
                    }
                }
                Tile::from_samples(TileCoord::new(0, column, 0), layout, samples).unwrap()
            })
            .collect();
        let output = stitch(extent, &tiles).unwrap();
        assert_eq!(output.dimensions(), (TILE_SIZE + 1, 2));
        assert_eq!(output.get_pixel(TILE_SIZE - 1, 0).0, [0, 10, 20]);
        assert_eq!(output.get_pixel(TILE_SIZE, 1).0, [2, 12, 22]);
    }

    #[test]
    fn rejects_non_rgb_tiles() {
        let tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(1, 1),
                halo: 0,
                channels: 1,
            },
            vec![1u8],
        )
        .unwrap();
        assert!(stitch(Extent::new(1, 1), &[tile]).is_err());
    }

    #[test]
    fn all_exif_orientations_place_pixels_correctly() {
        let input = RgbImage::from_fn(2, 3, |x, y| image::Rgb([(y * 2 + x + 1) as u8, 0, 0]));
        let expected = [
            vec![1, 2, 3, 4, 5, 6],
            vec![2, 1, 4, 3, 6, 5],
            vec![6, 5, 4, 3, 2, 1],
            vec![5, 6, 3, 4, 1, 2],
            vec![1, 3, 5, 2, 4, 6],
            vec![5, 3, 1, 6, 4, 2],
            vec![6, 4, 2, 5, 3, 1],
            vec![2, 4, 6, 1, 3, 5],
        ];
        for (i, pixels) in expected.iter().enumerate() {
            let output = orient(input.clone(), (i + 1) as u16);
            assert_eq!(output.pixels().map(|p| p[0]).collect::<Vec<_>>(), *pixels);
            assert_eq!(output.dimensions(), if i < 4 { (2, 3) } else { (3, 2) });
        }
    }

    #[test]
    fn preview_bounds_preserve_aspect_without_upscaling() {
        assert_eq!(fit(RgbImage::new(800, 400), 200).dimensions(), (200, 100));
        assert_eq!(fit(RgbImage::new(20, 10), 200).dimensions(), (20, 10));
        assert_eq!(fit(RgbImage::new(1, 800), 1).dimensions(), (1, 1));
    }

    #[test]
    fn embedded_jpeg_decodes_and_corruption_requests_fallback() {
        let jpeg = previews::Jpeg.encode(&RgbImage::new(8, 4)).unwrap();
        assert_eq!(decode_embedded(&jpeg, 6, 4).unwrap().dimensions(), (2, 4));
        assert!(decode_embedded(b"broken jpeg", 1, 4).is_none());
    }

    #[test]
    fn rejects_invalid_options_before_opening_source() {
        let missing = Path::new("missing-source.dng");
        let out = Path::new("unused.png");
        assert!(
            render(missing, out, 255, &DevelopSettings::default())
                .unwrap_err()
                .to_string()
                .contains("level")
        );
        assert!(
            preview(missing, out, 0)
                .unwrap_err()
                .to_string()
                .contains("max")
        );
    }
}
