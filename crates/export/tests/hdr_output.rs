use engine_api::recipe::Recipe;
use export::{ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, Metadata, export_one};
use pipeline_cpu::{Image, RenderSource};

fn fixture() -> (Image, Recipe) {
    let source = Image::new(64, 32, vec![vec![4.0; 64 * 32]; 3]).unwrap();
    let mut recipe = Recipe::default();
    recipe
        .edit(engine_api::recipe::EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.0;
        })
        .unwrap();
    (source, recipe)
}

#[test]
fn png_hdr_is_16bit_rec2020_with_transfer_and_no_sdr_icc() {
    for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
        let (source, recipe) = fixture();
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
                format: Format::Png,
                hdr: Some(transfer),
                color_space: ColorSpace::Rec2020,
                metadata: Metadata::None,
                output_dir: dir.path().into(),
                ..Default::default()
            },
        )
        .unwrap();
        let data = std::fs::read(path).unwrap();
        let mut chunks = Vec::new();
        let mut at = 8;
        while at < data.len() {
            let n = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            chunks.push((&data[at + 4..at + 8], &data[at + 8..at + 8 + n]));
            at += 12 + n;
        }
        let cicp = chunks
            .iter()
            .find(|(k, _)| *k == b"cICP")
            .expect("HDR cICP")
            .1;
        assert_eq!(
            cicp,
            &[9, if transfer == HdrTransfer::Pq { 16 } else { 18 }, 0, 1]
        );
        assert!(!chunks.iter().any(|(k, _)| *k == b"iCCP" || *k == b"sRGB"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(data))
            .read_info()
            .unwrap();
        assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
        let mut samples = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut samples).unwrap();
        let code = f64::from(u16::from_be_bytes(samples[..2].try_into().unwrap())) / 65535.0;
        // Independent inverse of ST 2084 or BT.2100 HLG (1000-nit display,
        // gamma 1.2). SDR diffuse white is 203 cd/m².
        let linear = match transfer {
            HdrTransfer::Pq => {
                let v = code.powf(32.0 / 2523.0);
                ((v - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * v))
                    .powf(16384.0 / 2610.0)
                    * 10000.0
                    / 203.0
            }
            HdrTransfer::Hlg => {
                let scene = if code <= 0.5 {
                    code * code / 3.0
                } else {
                    (((code - 0.55991073) / 0.17883277).exp() + 0.28466892) / 12.0
                };
                scene.powf(1.2) * 1000.0 / 203.0
            }
        };
        let a = pipeline_cpu::hdr_sigmoid_ln_a(4.0).exp();
        let expected =
            4.0 / (1.0 + (a / 4.0).powf(pipeline_cpu::SigmoidSettings::default().contrast));
        assert!(linear > 1.0, "highlights clipped: {linear}");
        assert!(
            (linear - f64::from(expected)).abs() < 0.003,
            "{transfer:?}: {linear} != {expected}"
        );
    }
}
