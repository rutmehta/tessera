//! Bridge regressions use generated LinearRaw pixels and temporary support roots.
use engine_api::recipe::{LocalAdjustment, LocalParams, MaskComponent, MaskKind, Recipe};
use std::{path::PathBuf, sync::Arc};
use tessera_ffi::*;

struct Fixture {
    root: tempfile::TempDir,
    engine: Arc<Engine>,
    path: PathBuf,
    support: PathBuf,
    id: String,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let photos = root.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let path = photos.join("synthetic.dng");
        std::fs::write(&path, bytes).unwrap();
        let support = root.path().join("support");
        let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        Self {
            root,
            engine,
            path,
            support,
            id,
        }
    }
    fn print(&self) -> Result<PrintImage, BridgeError> {
        self.engine.render_for_print(
            PrintRenderRequest {
                image_id: self.id.clone(),
                max_width: 32,
                max_height: 32,
                sharpening: PrintSharpening::None,
                profile: None,
            },
            None,
        )
    }
    fn export(&self) -> ExportReport {
        self.engine.export_batch(ExportTarget::Images { image_ids: vec![self.id.clone()] }, serde_json::json!({"format":"png", "destination":self.root.path().join("output"), "metadata":"none", "resize":{"mode":"none"}}).to_string(), None, None).unwrap()
    }
}

#[test]
fn native_proxy_print_and_export_ignore_malformed_embedded_profile_metadata() {
    let mut bytes =
        include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng").to_vec();
    let count = u16::from_le_bytes(bytes[38..40].try_into().unwrap()) as usize;
    let offset = (0..count)
        .map(|i| 40 + 12 * i)
        .find(|&p| u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) == 50778)
        .unwrap();
    // CalibrationIlluminant is profile-only metadata. Its invalid indirect
    // payload must not invalidate otherwise decodable Native proxy pixels.
    bytes[offset + 4..offset + 8].copy_from_slice(&3_u32.to_le_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        image_core::pipeline_adobe::dcp::read_embedded_profile(&mut std::io::Cursor::new(&bytes))
            .is_err()
    );
    assert!(
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes))
            .unwrap()
            .is_some()
    );
    let f = Fixture::new(&bytes);
    let printed = f
        .print()
        .expect("Native print must not parse the Adobe profile");
    assert!(!printed.data.is_empty());
    let report = f.export();
    assert_eq!(
        (report.exported, report.failed),
        (1, 0),
        "Native export must not parse the Adobe profile"
    );
}

#[test]
fn cached_proxy_depth_range_does_not_load_a_segmentation_model_through_ffi() {
    let bytes = include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng");
    let f = Fixture::new(bytes);
    let baseline = f.print().unwrap();
    let proxy = pipeline_cpu::CameraLinearProxy::from_dng(
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let input = pipeline_cpu::render_linear_before_geometry(
        &Default::default(),
        &pipeline_cpu::RenderSource::CameraLinear(&proxy),
        None,
    )
    .unwrap();
    let shown = image_core::depth::model_input(&input).unwrap();
    let store =
        image_core::ml_depth::DepthStore::new(f.support.join("previews/depth-cache"), 0).unwrap();
    image_core::ml_depth::DepthMap::from_normalized_inverse(
        input.width(),
        input.height(),
        vec![1.; (input.width() * input.height()) as usize],
    )
    .unwrap()
    .store_pinned(
        &store,
        &image_core::ml_depth::cache_key(&shown, image_core::ml_depth::MODEL_VERSION),
    )
    .unwrap();
    let mut recipe = Recipe::new(f.id.parse().unwrap());
    recipe.settings.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Depth {
            range: [0., 0.5],
            feather: 0.,
            model: None,
        })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    recipe.history.base = recipe.settings.clone();
    sidecar::Sidecar::write_recipe(
        sidecar::Sidecar::paths(&f.path).recipe,
        &sidecar::RecipeDocument {
            recipe,
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::create_dir_all(f.support.join("models/models.toml")).unwrap();
    let printed = f
        .print()
        .expect("cached depth must not load segmentation weights");
    assert_ne!(printed.data, baseline.data);
    let report = f.export();
    assert_eq!((report.exported, report.failed), (1, 0));
}
