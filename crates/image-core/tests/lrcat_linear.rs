#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use engine_api::{id::ImageId, jobs::CancellationToken, recipe::{DevelopSettings, ProcessVersion}, tile::TileCoord};
use image_core::{RawImage, Renderer, RenderOutput};
#[test]
fn classic_linear_dng_enters_camera_develop_for_native_and_adobe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    let image = RawImage::open(ImageId(818), &path).unwrap();
    assert_eq!(image.metadata().orientation, 6);
    assert_eq!(image.metadata().as_shot_wb[..3], [2.,1.,1.5]);
    let settings = DevelopSettings::default();
    for process in [ProcessVersion::NATIVE_CURRENT, ProcessVersion { family: engine_api::recipe::ProcessFamily::Adobe, revision: 6 }] {
        let renderer = Renderer::new(Default::default()).for_process_version(process);
        let mut tiles = Vec::new();
        renderer.render_tiles(&image, &settings, &[TileCoord::new(0,0,0)], RenderOutput::SceneLinear, &CancellationToken::new(), &mut |tile| tiles.push(tile)).unwrap();
        assert!(!tiles.is_empty());
    }
}
