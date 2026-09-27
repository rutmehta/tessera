use engine_api::recipe::{EditMeta, Recipe};
use export::{ColorSpace, ExportImage, ExportSettings, Format, HdrTransfer, export_one};
use pipeline_cpu::{Image, RenderSource};

fn boxes(data: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let n = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        assert!(n >= 8 && at + n <= data.len());
        result.push((&data[at + 4..at + 8], &data[at + 8..at + n]));
        at += n;
    }
    result
}

#[test]
fn avif_hdr_has_consistent_rec2020_transfer_properties() {
    for bits in [10, 12] {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let source = Image::new(64, 32, vec![vec![4.0; 64 * 32]; 3]).unwrap();
            let mut recipe = Recipe::default();
            recipe
                .edit(EditMeta::user("HDR", 0), |s| {
                    s.output.hdr = true;
                    s.output.hdr_headroom_stops = 2.0;
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
                    format: Format::Avif(export::AvifOptions {
                        bits,
                        quality: 100,
                        speed: 10,
                    }),
                    hdr: Some(transfer),
                    color_space: ColorSpace::Rec2020,
                    output_dir: dir.path().into(),
                    ..Default::default()
                },
            )
            .unwrap();
            let data = std::fs::read(&path).unwrap();
            let top = boxes(&data);
            let meta = top.iter().find(|(k, _)| *k == b"meta").unwrap().1;
            let meta = boxes(&meta[4..]);
            let iprp = boxes(meta.iter().find(|(k, _)| *k == b"iprp").unwrap().1);
            let props = boxes(iprp.iter().find(|(k, _)| *k == b"ipco").unwrap().1);
            let colors: Vec<_> = props.iter().filter(|(k, _)| *k == b"colr").collect();
            assert_eq!(colors.len(), 1, "no contradictory SDR ICC");
            assert_eq!(
                colors[0].1,
                &[
                    b'n',
                    b'c',
                    b'l',
                    b'x',
                    0,
                    9,
                    0,
                    if transfer == HdrTransfer::Pq { 16 } else { 18 },
                    0,
                    0,
                    128
                ]
            );
            assert_eq!(
                props.iter().find(|(k, _)| *k == b"pixi").unwrap().1,
                &[0, 0, 0, 0, 3, bits, bits, bits]
            );
            let ipma = iprp.iter().find(|(k, _)| *k == b"ipma").unwrap().1;
            assert_eq!(&ipma[8..], &[0, 1, 4, 1, 0x82, 3, 4]);
            #[cfg(target_os = "macos")]
            {
                let decoded = color_mgmt::decode_to_tiff(&data).unwrap();
                let mut decoder =
                    tiff::decoder::Decoder::new(std::io::Cursor::new(decoded)).unwrap();
                assert_eq!(decoder.dimensions().unwrap(), (64, 32));
                let samples = decoder.read_image().unwrap();
                assert!(
                    matches!(samples, tiff::decoder::DecodingResult::U16(_)),
                    "{samples:?}"
                );
            }
        }
    }
}
