//! M3-15 bridge surface: keyword suggestions, captions / alt text and OCR as
//! background jobs with the deterministic test models (no weights, no
//! network), cached results, accept (catalog-only or XMP opt-in), reject,
//! search over captions and OCR text, and the AI metadata settings.
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tessera_ffi::*;

struct Shoot {
    _dir: tempfile::TempDir,
    engine: Arc<Engine>,
    store: Arc<LibraryStore>,
    folder: String,
    /// (file stem, image id) sorted by stem.
    ids: Vec<(String, String)>,
}

/// Three flat JPEGs: blue, green, red. A keyword tree with Colors > Blue
/// (synonym "azure" on Green, to show synonym mapping).
fn shoot() -> Shoot {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (name, rgb) in [
        ("a_blue", [40, 80, 200]),
        ("b_green", [40, 180, 60]),
        ("c_red", [210, 40, 40]),
    ] {
        image::RgbImage::from_pixel(96, 64, image::Rgb(rgb))
            .save(photos.join(format!("{name}.jpg")))
            .unwrap();
    }
    let library = photos.join("library.json");
    let mut doc = library::Library::default();
    doc.add_keyword("Colors", None).unwrap();
    doc.add_keyword("Blue", Some("Colors")).unwrap();
    doc.write(&library).unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap()
        .path;
    let mut ids: Vec<(String, String)> = engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .map(|i| {
            let stem = Path::new(&i.path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (stem, i.id)
        })
        .collect();
    ids.sort();
    let store = engine
        .clone()
        .open_library(library.to_string_lossy().into_owned())
        .unwrap();
    engine.use_test_understanding().unwrap();
    Shoot {
        _dir: dir,
        engine,
        store,
        folder,
        ids,
    }
}

impl Shoot {
    fn all(&self) -> Vec<String> {
        self.ids.iter().map(|(_, id)| id.clone()).collect()
    }
    fn id(&self, stem: &str) -> String {
        self.ids.iter().find(|(s, _)| s == stem).unwrap().1.clone()
    }
    fn wait(&self, job: u64) -> UnderstandingJobInfo {
        let start = Instant::now();
        loop {
            let info = self
                .engine
                .understanding_jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .expect("job listed");
            if !matches!(
                info.state,
                UnderstandingJobState::Queued | UnderstandingJobState::Running
            ) {
                return info;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "job timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn search(&self, text: &str) -> Vec<String> {
        let mut ids = self
            .store
            .search(SearchRequest {
                text: text.into(),
                filters: vec![],
                scope: SearchScope::All,
                folder: Some(self.folder.clone()),
            })
            .unwrap()
            .image_ids;
        ids.sort();
        ids
    }
}

#[test]
fn suggestion_jobs_report_progress_cache_results_and_map_to_the_tree() {
    let s = shoot();
    let job = s.engine.clone().suggest_keywords(s.all()).unwrap();
    let info = s.wait(job);
    assert_eq!(info.state, UnderstandingJobState::Succeeded, "{info:?}");
    assert_eq!((info.done, info.total, info.failed), (3, 3, 0));
    assert_eq!(info.tasks, [UnderstandingTask::Keywords]);

    // Cached: a second request for the same model does no work.
    let again = s.wait(s.engine.clone().suggest_keywords(s.all()).unwrap());
    assert_eq!(
        (again.state, again.total),
        (UnderstandingJobState::Succeeded, 0)
    );
    // `force` recomputes.
    let forced = s.wait(
        s.engine
            .clone()
            .analyze_understanding(
                vec![s.id("a_blue")],
                vec![UnderstandingTask::Keywords],
                true,
            )
            .unwrap(),
    );
    assert_eq!(forced.total, 1);

    let blue = s.store.suggestions(vec![s.id("a_blue")]).unwrap();
    let names: Vec<_> = blue.iter().map(|k| k.keyword.as_str()).collect();
    assert_eq!(
        names,
        ["photograph", "blue", "gradient", "abstract", "texture"]
    );
    assert!(blue.windows(2).all(|w| w[0].confidence >= w[1].confidence));
    // "blue" maps onto the existing Colors > Blue; new concepts are proposed
    // under Suggested.
    let mapped = &blue[1];
    assert_eq!(mapped.path, ["Colors", "Blue"]);
    assert!(mapped.existing && !mapped.ambiguous);
    assert_eq!(blue[0].path, ["Suggested", "photograph"]);
    assert!(!blue[0].existing);

    // Merged over the selection: counts per keyword, colours once each.
    let merged = s.store.suggestions(s.all()).unwrap();
    let photograph = merged.iter().find(|k| k.keyword == "photograph").unwrap();
    assert_eq!(photograph.images, 3);
    assert_eq!(
        merged.iter().find(|k| k.keyword == "red").unwrap().images,
        1
    );

    // Suggesting never tags a photo.
    let meta = s.store.metadata(s.id("a_blue")).unwrap();
    assert!(meta.keywords.is_empty());
}

#[test]
fn accept_is_catalog_only_by_default_and_writes_xmp_when_opted_in() {
    let s = shoot();
    assert_eq!(
        s.engine.ai_metadata_settings().unwrap(),
        AiMetadataSettings::default()
    );
    s.wait(s.engine.clone().suggest_keywords(s.all()).unwrap());
    let blue = s.id("a_blue");
    let xmp = |id: &str| {
        let path = s
            .engine
            .list_images(ImageQuery::default())
            .unwrap()
            .into_iter()
            .find(|i| i.id == id)
            .unwrap()
            .path;
        std::fs::read_to_string(sidecar::Sidecar::paths(Path::new(&path)).xmp).unwrap_or_default()
    };

    // Accept "blue" and "photograph" for the whole selection: "blue" only
    // lands on the photo it was suggested for.
    let applied = s
        .store
        .accept_suggestions(s.all(), vec!["blue".into(), "photograph".into()])
        .unwrap();
    assert_eq!(applied, ["Colors|Blue", "Suggested|photograph"]);
    assert_eq!(s.search("keyword:Blue"), vec![blue.clone()]);
    assert_eq!(s.search("keyword:Suggested").len(), 3);
    let meta = s.store.metadata(blue.clone()).unwrap();
    assert!(meta.keywords.contains(&"Blue".to_owned()));
    assert!(!xmp(&blue).contains("photograph"), "catalog only");
    // Accepted suggestions leave the Suggested list.
    let left = s.store.suggestions(vec![blue.clone()]).unwrap();
    assert!(
        !left
            .iter()
            .any(|k| k.keyword == "blue" || k.keyword == "photograph")
    );
    // The tree gained Suggested > photograph.
    let tree = s.store.keywords(None).unwrap();
    assert!(
        tree.iter()
            .any(|k| k.name == "photograph" && k.parent.as_deref() == Some("Suggested"))
    );

    // Catalog-only tags survive a rescan, and removal forgets them.
    s.engine.index_folder(s.folder.clone()).unwrap();
    assert_eq!(s.search("keyword:photograph").len(), 3);
    s.store
        .apply_keywords(vec![blue.clone()], vec!["photograph".into()], false)
        .unwrap();
    assert_eq!(s.search("keyword:photograph").len(), 2);
    assert!(
        !s.store
            .metadata(blue.clone())
            .unwrap()
            .keywords
            .contains(&"photograph".to_owned())
    );

    // Opt in: accepted keywords go to XMP with their hierarchy.
    s.engine
        .set_ai_metadata_settings(AiMetadataSettings {
            auto_suggest_on_import: false,
            write_suggested_keywords_to_xmp: true,
        })
        .unwrap();
    assert!(
        s.engine
            .ai_metadata_settings()
            .unwrap()
            .write_suggested_keywords_to_xmp
    );
    s.store
        .accept_suggestions(vec![blue.clone()], vec!["gradient".into()])
        .unwrap();
    let packet = xmp(&blue);
    assert!(packet.contains("<rdf:li>gradient</rdf:li>"), "{packet}");
    assert!(
        packet.contains("<rdf:li>Suggested|gradient</rdf:li>"),
        "{packet}"
    );
    assert_eq!(s.search("keyword:gradient"), vec![blue]);
}

#[test]
fn rejected_suggestions_stay_rejected_until_reanalysed() {
    let s = shoot();
    s.wait(s.engine.clone().suggest_keywords(s.all()).unwrap());
    s.store
        .reject_suggestions(s.all(), vec!["TEXTURE".into()])
        .unwrap();
    assert!(
        !s.store
            .suggestions(s.all())
            .unwrap()
            .iter()
            .any(|k| k.keyword == "texture")
    );
    // Cached: a normal request keeps the rejection.
    s.wait(s.engine.clone().suggest_keywords(s.all()).unwrap());
    assert!(
        !s.store
            .suggestions(s.all())
            .unwrap()
            .iter()
            .any(|k| k.keyword == "texture")
    );
    // Accepting a rejected (absent) suggestion is a no-op, not a tag.
    assert!(
        s.store
            .accept_suggestions(s.all(), vec!["texture".into()])
            .unwrap()
            .is_empty()
    );
    s.wait(
        s.engine
            .clone()
            .analyze_understanding(s.all(), vec![UnderstandingTask::Keywords], true)
            .unwrap(),
    );
    assert!(
        s.store
            .suggestions(s.all())
            .unwrap()
            .iter()
            .any(|k| k.keyword == "texture")
    );
}

#[test]
fn captions_and_text_are_cached_searchable_and_keep_other_tasks() {
    let s = shoot();
    let green = s.id("b_green");
    let info = s.engine.image_understanding(green.clone()).unwrap();
    assert!(!info.has_caption && !info.has_ocr && info.caption.is_empty());
    s.wait(s.engine.clone().suggest_keywords(s.all()).unwrap());
    let job = s.wait(s.engine.clone().caption(s.all()).unwrap());
    assert_eq!((job.state, job.done), (UnderstandingJobState::Succeeded, 3));
    s.wait(s.engine.clone().ocr(vec![green.clone()]).unwrap());

    let info = s.engine.image_understanding(green.clone()).unwrap();
    assert!(info.has_keywords && info.has_caption && info.has_ocr);
    assert_eq!(info.caption, "A green gradient photograph.");
    assert!(info.alt_text.contains("green tones"));
    assert_eq!(info.ocr_text, "TEST CARD GREEN");
    assert_eq!(info.ocr.len(), 1);
    assert!(info.ocr[0].x0 < info.ocr[0].x1);
    // Running captions did not drop the cached suggestions.
    assert!(!s.store.suggestions(vec![green.clone()]).unwrap().is_empty());

    // Search: bare words and `text:` match generated captions and OCR text.
    assert_eq!(s.search("text:\"test card\""), vec![green.clone()]);
    assert_eq!(s.search("card"), vec![green.clone()]);
    assert_eq!(s.search("gradient").len(), 3);
    assert_eq!(s.search("text:\"red gradient\""), vec![s.id("c_red")]);
    assert!(s.search("text:\"test card\" AND NOT text:green").is_empty());

    // Generated text is a draft: nothing reached XMP.
    let meta = s.store.metadata(green.clone()).unwrap();
    assert!(meta.caption.is_empty() && meta.alt_text.is_empty());
    // Saving it (after editing) goes through the IPTC path, alt text included.
    s.store
        .set_iptc(
            vec![green.clone()],
            IptcEdit {
                caption: Some("Green card on a desk.".into()),
                alt_text: Some(info.alt_text.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let meta = s.store.metadata(green).unwrap();
    assert_eq!(meta.caption, "Green card on a desk.");
    assert_eq!(meta.alt_text, info.alt_text);
}

#[test]
fn jobs_cancel_and_auto_suggest_follows_the_setting() {
    let s = shoot();
    assert_eq!(s.engine.clone().auto_suggest(s.all()).unwrap(), None);
    s.engine
        .set_ai_metadata_settings(AiMetadataSettings {
            auto_suggest_on_import: true,
            write_suggested_keywords_to_xmp: false,
        })
        .unwrap();
    let auto = s
        .engine
        .clone()
        .auto_suggest(s.all())
        .unwrap()
        .expect("job");
    assert_eq!(s.wait(auto).done, 3);

    // Invalid input fails before a job is created.
    assert!(s.engine.clone().caption(vec!["nope".into()]).is_err());
    assert!(
        s.engine
            .clone()
            .analyze_understanding(s.all(), vec![], false)
            .is_err()
    );
    // Cancel: a queued or running job ends cancelled; nothing half-written.
    let job = s.engine.clone().ocr(s.all()).unwrap();
    s.engine.cancel_understanding_job(job).unwrap();
    let info = s.wait(job);
    assert!(
        matches!(
            info.state,
            UnderstandingJobState::Cancelled | UnderstandingJobState::Succeeded
        ),
        "{info:?}"
    );
    assert!(s.engine.cancel_understanding_job(9_999).is_err());
    let status = s.engine.understanding_model_status().unwrap();
    assert!(status.test_models);
}
