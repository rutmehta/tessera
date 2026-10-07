//! SP-INT2 (REV-SP-A S2, S3): documents and prints of a LinearRaw Smart
//! Preview neither drop retouch silently nor hide what the proxy omitted.
use engine_api::{
    id::RetouchId,
    recipe::{
        DevelopSettings, MaskComponent, MaskKind, Recipe,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
        settings::{LensProfileRef, LensProfileSource},
    },
};
use std::{path::PathBuf, sync::Arc};
use tessera_ffi::*;

struct Fixture {
    _root: tempfile::TempDir,
    engine: Arc<Engine>,
    path: PathBuf,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let photos = root.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let path = photos.join("synthetic.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let support = root.path().join("support");
        let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        Self {
            _root: root,
            engine,
            path,
            id,
        }
    }
    fn write(&self, settings: DevelopSettings) {
        let mut recipe = Recipe::new(self.id.parse().unwrap());
        recipe.settings = settings;
        recipe.history.base = recipe.settings.clone();
        sidecar::Sidecar::write_recipe(
            sidecar::Sidecar::paths(&self.path).recipe,
            &sidecar::RecipeDocument {
                recipe,
                ..Default::default()
            },
        )
        .unwrap();
    }
    fn document(&self) -> Result<Vec<f32>, BridgeError> {
        let s = self
            .engine
            .clone()
            .open_document_from_image(self.id.clone(), true)?;
        let (_, _, px) = s.read_level(0).unwrap();
        s.close();
        Ok(px)
    }
    fn print(&self) -> PrintImage {
        self.engine
            .render_for_print(
                PrintRenderRequest {
                    image_id: self.id.clone(),
                    max_width: 32,
                    max_height: 32,
                    sharpening: PrintSharpening::None,
                    profile: None,
                },
                None,
            )
            .unwrap()
    }
}

fn plain() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s
}

#[test]
fn sp_int2_proxy_document_renders_retouch_instead_of_dropping_it() {
    let f = Fixture::new();
    f.write(plain());
    let base = f.document().unwrap();
    let mut retouched = plain();
    retouched.locals.retouch.push(RetouchOperation {
        id: RetouchId(1),
        kind: RetouchKind::Clone {
            source_offset: [0.5, 0.],
        },
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.5, 1.]],
                    radius: 0.2,
                    feather: 0.,
                    ..Default::default()
                }],
            })],
        },
        opacity: 100.,
        feather: 0.,
        enabled: true,
    });
    f.write(retouched);
    match f.document() {
        Ok(pixels) => assert_ne!(pixels, base, "document open silently dropped retouch"),
        Err(e) => panic!("document open must render proxy retouch like Develop: {e}"),
    }
}

#[test]
fn sp_int2_proxy_print_reports_what_the_proxy_omitted() {
    let f = Fixture::new();
    f.write(plain());
    let printed = f.print();
    assert!(
        printed.notes.iter().any(|n| n.contains("Smart Preview")),
        "print must say it used the Smart Preview: {:?}",
        printed.notes
    );
    let mut s = plain();
    s.lens.profile = LensProfileSource::Database {
        profile: LensProfileRef::named("synthetic unavailable profile"),
    };
    f.write(s);
    let printed = f.print();
    assert!(
        printed
            .notes
            .iter()
            .any(|n| n.contains("Lens profile unavailable")),
        "print must surface the proxy's omissions as sentences: {:?}",
        printed.notes
    );
    assert!(
        printed.notes.iter().all(|n| !n.contains("/lens")),
        "notes are sentences, not JSON pointers: {:?}",
        printed.notes
    );
}
