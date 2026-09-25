use engine_api::{jobs::CancellationToken, recipe::Recipe};
use export::{ExportImage, ExportSettings, Metadata};
use pipeline_cpu::{Image, RenderSource};

#[test]
fn render_then_encode_is_owned_and_cancelled_without_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let rendered = {
        let source = Image::new(16, 16, vec![vec![0.1; 256]; 3]).unwrap();
        export::render_one_cancellable(
            &ExportImage {
                source: RenderSource::Rgb(&source),
                name: "staged",
                sequence: 1,
                date: "",
                metadata: None,
            },
            &Recipe::default(),
            &ExportSettings {
                output_dir: dir.path().into(),
                metadata: Metadata::All,
                ..Default::default()
            },
            &cancel,
            None,
            None,
        )
        .unwrap()
    };
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    cancel.cancel();
    let worker = std::thread::spawn(move || rendered.finish(&cancel));
    assert!(worker.join().unwrap().is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
