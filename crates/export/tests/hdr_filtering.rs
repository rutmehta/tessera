//! Output filters must respect the recipe's radiometric ceiling, not PQ's 10,000 nits.
use engine_api::recipe::Recipe;
use export::{
    ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, Metadata, Resize, SharpenAmount,
    SharpenFor, export_one,
};
use pipeline_cpu::{Image, RenderSource};

fn check_edge(resize: Resize, sharpen_for: SharpenFor) {
    let plane = (0..64 * 32)
        .map(|i| if i % 64 < 32 { 0.0 } else { 1000.0 })
        .collect::<Vec<_>>();
    let source = Image::new(64, 32, vec![plane; 3]).unwrap();
    let mut recipe = Recipe::default();
    recipe
        .edit(engine_api::recipe::EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.0;
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = export_one(
        &ExportImage {
            source: RenderSource::Rgb(&source),
            name: "edge",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &recipe,
        &ExportSettings {
            format: Format::Png,
            hdr: Some(HdrTransfer::Pq),
            color_space: ColorSpace::Rec2020,
            metadata: Metadata::None,
            output_dir: dir.path().into(),
            resize,
            sharpen_for,
            sharpen_amount: SharpenAmount::High,
            ..Default::default()
        },
    )
    .unwrap();
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .read_info()
        .unwrap();
    assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
    assert_eq!(reader.info().color_type, png::ColorType::Rgb);
    let mut bytes = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut bytes).unwrap();
    assert_eq!(
        (frame.width, frame.height),
        resize.dimensions(64, 32).unwrap()
    );
    let nits = bytes[..frame.buffer_size()]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            let code = f64::from(u16::from_be_bytes([b[0], b[1]])) / 65535.0;
            let p = code.powf(32.0 / 2523.0);
            10000.0
                * ((p - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p))
                    .powf(16384.0 / 2610.0)
        })
        .collect::<Vec<_>>();
    let peak = nits.iter().copied().fold(0.0, f64::max);
    // Allow PNG16 quantization, but not filter overshoot above 203 * 2^2 nits.
    assert!(
        peak <= 812.1,
        "{resize:?} {sharpen_for:?}: peak {peak} nits exceeds 812"
    );
    assert!(
        peak > 800.0,
        "highlights must retain two-stop headroom: {peak}"
    );
    assert!(nits.iter().any(|&v| v < 0.001), "black must remain black");
}

#[test]
fn pq_resize_respects_two_stop_headroom() {
    check_edge(Resize::Percent(73.0), SharpenFor::None);
}

#[test]
fn pq_sharpen_respects_two_stop_headroom() {
    check_edge(Resize::None, SharpenFor::Screen);
}

#[test]
fn pq_resize_then_sharpen_respects_two_stop_headroom() {
    check_edge(Resize::Percent(73.0), SharpenFor::Screen);
}
