//! Document exports retain color meaning, not just pixel bytes.
use compositor::{ColorProfile, Depth, DocState, Document, Layer, Rect};
use engine_api::document::{DocumentExportSettings, DocumentFormat};
use engine_api::id::LayerId;
use engine_api::tile::Extent;
use engine_api::tools::{
    DocumentToolCall, DocumentToolRequest, DocumentToolResponse, ExportFormat,
};
use image::ImageDecoder;
use tessera_mcp::Console;

fn document(depth: Depth, icc: Option<Vec<u8>>) -> Document {
    let extent = Extent::new(2, 2);
    let mut state = DocState::new(extent, depth);
    state.profile = icc.map(|icc| ColorProfile::from_icc("Test profile", icc));
    let mut layer = Layer::pixel("Color", extent, depth);
    layer.id = LayerId(1);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(extent), 1, |_, _, p| {
            p.copy_from_slice(&[0.25, 0.5, 0.75, 0.5]);
        })
        .unwrap();
    state.root.push(std::sync::Arc::new(layer));
    state.next_id = 2;
    Document::new(state)
}

fn export(
    console: &mut Console,
    id: engine_api::id::DocumentId,
    path: &std::path::Path,
    encoding: ExportFormat,
) {
    let response = console.execute_document(DocumentToolRequest {
        call: DocumentToolCall::ExportDocument {
            document: id,
            settings: DocumentExportSettings {
                path: path.to_str().unwrap().into(),
                format: DocumentFormat::Image {
                    encoding,
                    resize: None,
                    profile: None,
                },
            },
        },
        rationale: None,
        group: None,
        expect_head: None,
    });
    assert!(
        matches!(response, DocumentToolResponse::Ok(_)),
        "{response:?}"
    );
}

#[test]
fn integer_document_retains_embedded_profile_and_samples() {
    let dir = tempfile::tempdir().unwrap();
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::AdobeRgb)
        .unwrap();
    let input = dir.path().join("input.tessera-doc");
    compositor::format::save(
        &document(Depth::U16, Some(profile.icc_bytes().to_vec())),
        &input,
    )
    .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_document(&input).unwrap();
    let output = dir.path().join("out.png");
    export(
        &mut console,
        id,
        &output,
        ExportFormat::Png { bit_depth: 16 },
    );
    let icc = image::ImageReader::open(&output)
        .unwrap()
        .into_decoder()
        .unwrap()
        .icc_profile()
        .unwrap()
        .unwrap();
    assert_eq!(icc, profile.icc_bytes());
    assert_eq!(
        image::open(&output)
            .unwrap()
            .into_rgba16()
            .get_pixel(0, 0)
            .0,
        [16384, 32768, 49151, 32768]
    );
}

#[test]
fn float_document_encodes_its_profile_transfer_curve_without_changing_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::AdobeRgb)
        .unwrap();
    let input = dir.path().join("input.tessera-doc");
    compositor::format::save(
        &document(Depth::F32, Some(profile.icc_bytes().to_vec())),
        &input,
    )
    .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_document(&input).unwrap();
    let output = dir.path().join("out.png");
    export(
        &mut console,
        id,
        &output,
        ExportFormat::Png { bit_depth: 16 },
    );
    let px = image::open(&output)
        .unwrap()
        .into_rgba16()
        .get_pixel(0, 0)
        .0;
    for (actual, linear) in px[..3].iter().zip([0.25_f32, 0.5, 0.75]) {
        let expected = (linear.powf(256.0 / 563.0) * 65535.0).round() as i32;
        assert!(
            (i32::from(*actual) - expected).abs() <= 3,
            "{px:?} != {expected}"
        );
    }
    assert_eq!(px[3], 32768);
}

#[test]
fn flat_image_open_retains_embedded_profile_for_export() {
    use image::ImageEncoder;
    let dir = tempfile::tempdir().unwrap();
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::DisplayP3)
        .unwrap();
    let input = dir.path().join("input.png");
    let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&input).unwrap());
    encoder
        .set_icc_profile(profile.icc_bytes().to_vec())
        .unwrap();
    encoder
        .write_image(&[64, 128, 192, 128], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_document(&input).unwrap();
    let output = dir.path().join("out.png");
    export(
        &mut console,
        id,
        &output,
        ExportFormat::Png { bit_depth: 8 },
    );
    let icc = image::ImageReader::open(&output)
        .unwrap()
        .into_decoder()
        .unwrap()
        .icc_profile()
        .unwrap()
        .unwrap();
    assert_eq!(icc, profile.icc_bytes());
    assert_eq!(
        image::open(&output).unwrap().into_rgba8().get_pixel(0, 0).0,
        [64, 128, 192, 128]
    );
}

#[test]
fn psd_import_captures_warnings_without_losing_preserved_records() {
    let dir = tempfile::tempdir().unwrap();
    let mut psd = compositor::psd::to_psd(&document(Depth::U8, None)).unwrap();
    psd.layer_section.layers[0].blend_mode = *b"test";
    let expected = compositor::psd::from_psd(&psd).unwrap().warnings;
    assert!(!expected.is_empty());
    for extension in ["psd", "psb"] {
        psd.version = if extension == "psb" {
            psd::Version::Psb
        } else {
            psd::Version::Psd
        };
        let input = dir.path().join(format!("input.{extension}"));
        std::fs::write(&input, psd.write().unwrap()).unwrap();
        let mut console = Console::open(dir.path().join(format!("app-{extension}"))).unwrap();
        let id = console.open_document(&input).unwrap();
        let session = console.documents().session(id).unwrap();
        assert_eq!(session.warnings, expected);
        let exported = compositor::psd::to_psd(session.document()).unwrap();
        assert_eq!(exported.layer_section.layers[0].blend_mode, *b"test");
    }
}

#[test]
fn untagged_float_export_keeps_srgb_transfer_and_linear_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tessera-doc");
    compositor::format::save(&document(Depth::F32, None), &input).unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_document(&input).unwrap();
    let output = dir.path().join("out.png");
    export(
        &mut console,
        id,
        &output,
        ExportFormat::Png { bit_depth: 16 },
    );
    let px = image::open(&output)
        .unwrap()
        .into_rgba16()
        .get_pixel(0, 0)
        .0;
    for (actual, linear) in px[..3].iter().zip([0.25_f32, 0.5, 0.75]) {
        let expected = ((1.055 * linear.powf(1.0 / 2.4) - 0.055) * 65535.0).round() as u16;
        assert_eq!(*actual, expected);
    }
    assert_eq!(px[3], 32768);
}

#[test]
fn invalid_profile_and_unresolved_output_handle_do_not_replace_destination() {
    for invalid in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.tessera-doc");
        compositor::format::save(
            &document(Depth::U8, invalid.then(|| b"not an ICC profile".to_vec())),
            &input,
        )
        .unwrap();
        let mut console = Console::open(dir.path().join("app")).unwrap();
        let id = console.open_document(&input).unwrap();
        let output = dir.path().join("out.png");
        std::fs::write(&output, b"keep original").unwrap();
        let response = console.execute_document(DocumentToolRequest {
            call: DocumentToolCall::ExportDocument {
                document: id,
                settings: DocumentExportSettings {
                    path: output.to_str().unwrap().into(),
                    format: DocumentFormat::Image {
                        encoding: ExportFormat::Png { bit_depth: 8 },
                        resize: None,
                        profile: (!invalid).then(|| {
                            engine_api::color::IccProfileHandle::from_profile_bytes(b"unresolved")
                        }),
                    },
                },
            },
            rationale: None,
            group: None,
            expect_head: None,
        });
        assert!(
            matches!(response, DocumentToolResponse::Error(_)),
            "{response:?}"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"keep original");
    }
}

#[test]
fn untagged_integer_exports_embed_srgb_in_every_supported_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tessera-doc");
    compositor::format::save(&document(Depth::U8, None), &input).unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_document(&input).unwrap();
    for (name, encoding) in [
        ("out.jpg", ExportFormat::Jpeg { quality: 100 }),
        ("out8.png", ExportFormat::Png { bit_depth: 8 }),
        ("out16.png", ExportFormat::Png { bit_depth: 16 }),
        ("out8.tiff", ExportFormat::Tiff { bit_depth: 8 }),
        ("out16.tiff", ExportFormat::Tiff { bit_depth: 16 }),
    ] {
        let output = dir.path().join(name);
        export(&mut console, id, &output, encoding);
        let icc = if name.ends_with("tiff") {
            let mut decoder =
                tiff::decoder::Decoder::new(std::fs::File::open(&output).unwrap()).unwrap();
            decoder
                .get_tag_u8_vec(tiff::tags::Tag::Unknown(34675))
                .unwrap()
        } else {
            image::ImageReader::open(&output)
                .unwrap()
                .into_decoder()
                .unwrap()
                .icc_profile()
                .unwrap()
                .expect("export must embed ICC")
        };
        assert_eq!(&icc[36..40], b"acsp");
        assert_eq!(&icc[16..20], b"RGB ");
    }
}
