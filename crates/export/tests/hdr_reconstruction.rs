//! Independent inverse-transfer checks on encoded files, not encoder buffers.
use engine_api::recipe::{EditMeta, Recipe};
use export::{ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};

fn inverse(v: [f64; 3], transfer: HdrTransfer) -> [f64; 3] {
    match transfer {
        HdrTransfer::Pq => v.map(|v| {
            let p = v.powf(32.0 / 2523.0);
            ((p - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p))
                .powf(16384.0 / 2610.0)
                * 10000.0
                / 203.0
        }),
        HdrTransfer::Hlg => {
            let scene = v.map(|v| {
                if v <= 0.5 {
                    v * v / 3.0
                } else {
                    (((v - 0.55991073) / 0.17883277).exp() + 0.28466892) / 12.0
                }
            });
            let y = 0.2627 * scene[0] + 0.6780 * scene[1] + 0.0593 * scene[2];
            scene.map(|v| v * y.powf(0.2) * 1000.0 / 203.0)
        }
    }
}

fn samples(bytes: &[u8], format: Format) -> Vec<u16> {
    if matches!(format, Format::Png) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut out = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut out).unwrap();
        return out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_be_bytes(*v))
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        let tiff = color_mgmt::decode_to_tiff(bytes).unwrap();
        let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(tiff)).unwrap();
        let tiff::decoder::DecodingResult::U16(samples) = decoder.read_image().unwrap() else {
            panic!("16-bit ImageIO decode required")
        };
        let channels = samples.len() / (32 * 32);
        samples
            .chunks_exact(channels)
            .flat_map(|p| p[..3].iter().copied())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    panic!("AVIF reconstruction requires ImageIO");
}

#[test]
fn colored_highlights_reconstruct_and_follow_recipe_headroom() {
    let mut formats = vec![Format::Png];
    if cfg!(target_os = "macos") {
        for bits in [10, 12] {
            formats.push(Format::Avif(export::AvifOptions {
                bits,
                quality: 100,
                speed: 10,
            }));
        }
    }
    for format in formats {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            for (enabled, stops) in [(false, 2.0), (true, 0.0), (true, 2.0), (true, 16.0)] {
                for rgb in [
                    [0.0, 0.0, 0.0],
                    [0.18, 0.18, 0.18],
                    [2.0, 1.4, 0.9],
                    [20.0, 1.0, 1.0],
                    [-0.1, 1.0, 0.5],
                ] {
                    let source =
                        Image::new(32, 32, rgb.map(|v| vec![v as f32; 32 * 32]).to_vec()).unwrap();
                    let mut recipe = Recipe::default();
                    recipe
                        .edit(EditMeta::user("HDR", 0), |s| {
                            s.output.hdr = enabled;
                            s.output.hdr_headroom_stops = stops;
                        })
                        .unwrap();
                    let dir = tempfile::tempdir().unwrap();
                    let path = export_one(
                        &ExportImage {
                            source: RenderSource::Rgb(&source),
                            name: "hdr",
                            sequence: 1,
                            date: "",
                            metadata: None,
                        },
                        &recipe,
                        &ExportSettings {
                            format,
                            hdr: Some(transfer),
                            color_space: ColorSpace::Rec2020,
                            metadata: Metadata::None,
                            output_dir: dir.path().into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let bytes = std::fs::read(path).unwrap();
                    let codes = samples(&bytes, format);
                    let v =
                        std::array::from_fn(|c| f64::from(codes[(16 * 32 + 16) * 3 + c]) / 65535.0);
                    let actual = inverse(v, transfer);
                    let h = if enabled {
                        f64::from(stops).exp2()
                    } else {
                        1.0
                    }
                    .min(if transfer == HdrTransfer::Pq {
                        10000.0 / 203.0
                    } else {
                        1000.0 / 203.0
                    });
                    let y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
                    let a = 0.18 * (h / 0.18 - 1.0).powf(1.0 / 1.5);
                    let gain = if y > 0.0 {
                        h / (1.0 + (a / y).powf(1.5)) / y
                    } else {
                        0.0
                    };
                    let toned = rgb.map(|v| v * gain);
                    let grey = y * gain;
                    let max = if transfer == HdrTransfer::Hlg {
                        h.min(1000.0 / 203.0 * (grey * 203.0 / 1000.0).powf(1.0 / 6.0))
                    } else {
                        h
                    };
                    let mut chroma = 1.0f64;
                    for c in toned {
                        if c < 0.0 {
                            chroma = chroma.min(-grey / (c - grey));
                        }
                        if c > max {
                            chroma = chroma.min((max - grey) / (c - grey));
                        }
                    }
                    let expected = toned.map(|v| (grey + chroma * (v - grey)).clamp(0.0, h));
                    for (got, want) in actual.into_iter().zip(expected) {
                        // PQ has a steep inverse near 10,000 nits: a single
                        // 12-bit code is about 0.23% there. Bound radiometric
                        // error by precision, rather than an SDR-sized constant.
                        let tolerance = match format {
                            Format::Png => 0.003,
                            Format::Avif(export::AvifOptions { bits: 12, .. }) => {
                                0.004 * want.max(1.0)
                            }
                            _ => 0.015 * want.max(1.0),
                        };
                        assert!(
                            (got - want).abs() < tolerance,
                            "{format:?} {transfer:?} {enabled} {stops}: {got} != {want}; codes={v:?}"
                        );
                    }
                }
            }
        }
    }
}
