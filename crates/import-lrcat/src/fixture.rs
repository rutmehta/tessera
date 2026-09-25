//! A synthetic Lightroom Classic catalog with real JPEG originals and a
//! `Previews.lrdata` cache, for end-to-end tests and the app's acceptance walk
//! (`tessera import lrcat --make-fixture <dir>`).
//!
//! Tables and columns follow Lightroom's names; the data are invented. The
//! catalog records its root as `/Volumes/Old Drive/Photos/` (a drive that is no
//! longer mounted), so the photos written under `<dir>/Photos` must be located
//! by relocating that root. The "Lightroom previews" are this module's own
//! approximation of the edits (exposure gain, desaturation), not Adobe renders.
use crate::previews::{Section, write_lrprev};
use engine_api::error::{EngineError, EngineResult};
use image::{ImageEncoder, RgbImage, codecs::jpeg::JpegEncoder};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Root folder recorded in the catalog.
pub const MOVED_ROOT: &str = "/Volumes/Old Drive/Photos/";

#[derive(Debug, Clone)]
pub struct Fixture {
    /// `<dir>/Catalog/Fixture.lrcat`
    pub catalog: PathBuf,
    /// `<dir>/Photos`: where the root actually is now.
    pub photos: PathBuf,
    /// `<dir>/Catalog/Fixture Previews.lrdata`
    pub previews: PathBuf,
}

struct Photo {
    id: i64,
    file: i64,
    folder: i64,
    name: &'static str,
    /// Written to disk (false: the catalog references a missing original).
    present: bool,
    pick: i64,
    rating: i64,
    label: &'static str,
    exposure: f32,
    saturation: f32,
    hue: f32,
}

const PHOTOS: &[Photo] = &[
    Photo {
        id: 30,
        file: 20,
        folder: 10,
        name: "ceremony-01",
        present: true,
        pick: 1,
        rating: 5,
        label: "Red",
        exposure: 0.5,
        saturation: 0.0,
        hue: 0.05,
    },
    Photo {
        id: 31,
        file: 21,
        folder: 10,
        name: "ceremony-02",
        present: true,
        pick: -1,
        rating: 0,
        label: "",
        exposure: 0.0,
        saturation: 0.0,
        hue: 0.15,
    },
    Photo {
        id: 32,
        file: 22,
        folder: 10,
        name: "ceremony-03",
        present: true,
        pick: 0,
        rating: 3,
        label: "Client",
        exposure: -1.0,
        saturation: 0.0,
        hue: 0.3,
    },
    Photo {
        id: 33,
        file: 23,
        folder: 11,
        name: "portrait-01",
        present: true,
        pick: 0,
        rating: 2,
        label: "Red",
        exposure: 1.0,
        saturation: 0.0,
        hue: 0.55,
    },
    Photo {
        id: 34,
        file: 24,
        folder: 11,
        name: "portrait-02",
        present: true,
        pick: 0,
        rating: 0,
        label: "",
        exposure: 0.0,
        saturation: 0.0,
        hue: 0.7,
    },
    Photo {
        id: 35,
        file: 25,
        folder: 11,
        name: "lost-01",
        present: false,
        pick: 0,
        rating: 4,
        label: "",
        exposure: 0.0,
        saturation: 0.0,
        hue: 0.85,
    },
];
/// Virtual copy of image 30 ("Black & White").
const COPY: i64 = 36;

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("fixture", e.to_string())
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// A gradient sky, a horizon band and a disc: smooth areas and edges.
fn original(hue: f32, width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
        let (dx, dy) = (u - 0.62, (v - 0.42) * height as f32 / width as f32);
        let rgb = if dx * dx + dy * dy < 0.012 {
            hsv((hue + 0.5) % 1.0, 0.7, 0.9)
        } else if v > 0.65 {
            hsv((hue + 0.08) % 1.0, 0.55, 0.25 + 0.3 * u)
        } else {
            hsv(hue, 0.35 + 0.3 * v, 0.95 - 0.5 * v)
        };
        image::Rgb(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
    })
}

fn to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let e = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (e * 255.0).round() as u8
}

/// Stand-in for Lightroom's rendering: exposure as a linear gain, then saturation.
fn simulated_render(img: &RgbImage, exposure: f32, saturation: f32) -> RgbImage {
    let gain = 2f32.powf(exposure);
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let lin = p.0.map(|c| to_linear(c) * gain);
        let y = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
        let k = 1.0 + saturation / 100.0;
        p.0 = lin.map(|c| to_srgb(y + (c - y) * k));
    }
    out
}

fn jpeg(img: &RgbImage) -> EngineResult<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 92)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(err)?;
    Ok(out)
}

fn develop_xmp(exposure: f32, saturation: f32, extra: &str) -> String {
    format!(
        r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="{exposure:+.2}" crs:Saturation="{saturation:.0}"{extra}/>"#
    )
}

/// Write the fixture under `dir` (created if needed; existing fixture files are
/// replaced). Returns the catalog, photo and preview paths.
pub fn write(dir: &Path) -> EngineResult<Fixture> {
    let catalog_dir = dir.join("Catalog");
    let photos = dir.join("Photos");
    for d in [
        catalog_dir.clone(),
        photos.join("2026/wedding"),
        photos.join("2026/portraits"),
    ] {
        std::fs::create_dir_all(&d).map_err(|e| EngineError::io_at(&d, &e))?;
    }
    let catalog = catalog_dir.join("Fixture.lrcat");
    let previews = catalog_dir.join("Fixture Previews.lrdata");
    for stale in [catalog.clone(), previews.join("previews.db")] {
        let _ = std::fs::remove_file(stale);
    }
    let c = Connection::open(&catalog).map_err(err)?;
    c.execute_batch(&format!(r#"
CREATE TABLE Adobe_variablesTable(id_local INTEGER PRIMARY KEY, name TEXT, value TEXT);
INSERT INTO Adobe_variablesTable(name, value) VALUES('Adobe_DBVersion','1300025');
CREATE TABLE AgLibraryRootFolder(id_local INTEGER PRIMARY KEY, absolutePath TEXT, name TEXT);
INSERT INTO AgLibraryRootFolder VALUES(1,'{MOVED_ROOT}','Photos');
CREATE TABLE AgLibraryFolder(id_local INTEGER PRIMARY KEY, rootFolder INTEGER, pathFromRoot TEXT);
INSERT INTO AgLibraryFolder VALUES(10,1,'2026/wedding/'),(11,1,'2026/portraits/'),(12,1,'2026/');
CREATE TABLE AgLibraryFile(id_local INTEGER PRIMARY KEY, folder INTEGER, baseName TEXT, extension TEXT);
CREATE TABLE Adobe_images(id_local INTEGER PRIMARY KEY, rootFile INTEGER, masterImage INTEGER, copyName TEXT, orientation TEXT, captureTime TEXT, pick INTEGER, rating INTEGER, colorLabels TEXT);
CREATE TABLE Adobe_imageDevelopSettings(image INTEGER, text TEXT, processVersion TEXT);
CREATE TABLE AgLibraryKeyword(id_local INTEGER PRIMARY KEY, name TEXT, parent INTEGER);
INSERT INTO AgLibraryKeyword VALUES(1,'Places',NULL),(2,'NYC',1),(3,'People',NULL),(4,'Alice',3),(5,'Trips',NULL),(6,'Paris',5),(7,'Paris',1);
CREATE TABLE AgLibraryKeywordSynonym(keyword INTEGER, name TEXT);
INSERT INTO AgLibraryKeywordSynonym VALUES(2,'New York');
CREATE TABLE AgLibraryKeywordImage(image INTEGER, tag INTEGER);
INSERT INTO AgLibraryKeywordImage VALUES(30,2),(31,2),(33,4),(34,6);
CREATE TABLE AgLibraryCollection(id_local INTEGER PRIMARY KEY, name TEXT, parent INTEGER, creationId TEXT);
INSERT INTO AgLibraryCollection VALUES
 (1,'Wedding',NULL,'com.adobe.ag.library.collection_set'),
 (2,'Selects',1,'com.adobe.ag.library.collection'),
 (3,'Four stars and up',1,'com.adobe.ag.library.smart_collection'),
 (4,'Client review',NULL,'com.adobe.ag.library.collection'),
 (5,'Blue label',NULL,'com.adobe.ag.library.smart_collection');
CREATE TABLE AgLibraryCollectionImage(collection INTEGER, image INTEGER, position REAL);
INSERT INTO AgLibraryCollectionImage VALUES(2,30,1),(2,{COPY},2),(2,33,3),(4,31,1),(4,32,2),(4,35,3);
CREATE TABLE AgLibraryCollectionContent(collection INTEGER, content TEXT);
INSERT INTO AgLibraryCollectionContent VALUES
 (3,'s = {{ combine = "intersect", {{ criteria = "rating", operation = ">=", value = 3 }} }}'),
 (5,'s = {{ combine = "intersect", {{ criteria = "labelColor", operation = "==", value = "blue" }} }}');
CREATE TABLE Adobe_libraryImageDevelopHistoryStep(id_local INTEGER PRIMARY KEY, image INTEGER, name TEXT, text TEXT, dateCreated REAL);
CREATE TABLE Adobe_libraryImageDevelopSnapshot(id_local INTEGER PRIMARY KEY, image INTEGER, name TEXT, text TEXT);
CREATE TABLE AgLibraryFolderStack(id_local INTEGER PRIMARY KEY, folder INTEGER);
INSERT INTO AgLibraryFolderStack VALUES(1,10);
CREATE TABLE AgLibraryFolderStackImage(stack INTEGER, image INTEGER, position INTEGER);
INSERT INTO AgLibraryFolderStackImage VALUES(1,30,1),(1,{COPY},2);
CREATE TABLE AgLibraryFace(id_local INTEGER PRIMARY KEY, image INTEGER, cluster INTEGER, x REAL, y REAL, width REAL, height REAL);
INSERT INTO AgLibraryFace VALUES(1,33,1,0.4,0.2,0.2,0.3);
CREATE TABLE AgLibraryFaceCluster(id_local INTEGER PRIMARY KEY, name TEXT);
INSERT INTO AgLibraryFaceCluster VALUES(1,'Alice');
CREATE TABLE AgLibraryKeywordFace(face INTEGER, keyword INTEGER);
INSERT INTO AgLibraryKeywordFace VALUES(1,4);
CREATE TABLE AgHarvestedExifMetadata(image INTEGER, gpsLatitude REAL, gpsLongitude REAL);
INSERT INTO AgHarvestedExifMetadata VALUES(30,40.7,-74.0);
"#)).map_err(err)?;

    std::fs::create_dir_all(&previews).map_err(|e| EngineError::io_at(&previews, &e))?;
    let pc = Connection::open(previews.join("previews.db")).map_err(err)?;
    pc.execute_batch(
        "CREATE TABLE ImageCacheEntry(id_local INTEGER PRIMARY KEY, imageId INTEGER, uuid TEXT, digest TEXT, orientation TEXT);",
    )
    .map_err(err)?;

    let folder_path = |folder: i64| {
        if folder == 10 {
            "2026/wedding"
        } else {
            "2026/portraits"
        }
    };
    let mut renders = Vec::new();
    for p in PHOTOS {
        c.execute(
            "INSERT INTO AgLibraryFile VALUES(?1,?2,?3,'jpg')",
            rusqlite::params![p.file, p.folder, p.name],
        )
        .map_err(err)?;
        c.execute(
            "INSERT INTO Adobe_images VALUES(?1,?2,NULL,NULL,'AB',?3,?4,?5,?6)",
            rusqlite::params![
                p.id,
                p.file,
                format!("2026-06-{:02}T14:00:00", p.id - 20),
                p.pick,
                p.rating,
                p.label
            ],
        )
        .map_err(err)?;
        let src = original(p.hue, 900, 600);
        if p.present {
            let path = photos
                .join(folder_path(p.folder))
                .join(format!("{}.jpg", p.name));
            std::fs::write(&path, jpeg(&src)?).map_err(|e| EngineError::io_at(&path, &e))?;
        }
        if p.exposure != 0.0 || p.id == 30 {
            let extra = if p.id == 30 {
                r#" crs:Contrast2012="+10" crs:FutureKnob="42""#
            } else {
                ""
            };
            c.execute(
                "INSERT INTO Adobe_imageDevelopSettings VALUES(?1,?2,'15.4')",
                rusqlite::params![p.id, develop_xmp(p.exposure, p.saturation, extra)],
            )
            .map_err(err)?;
            c.execute(
                "INSERT INTO Adobe_libraryImageDevelopHistoryStep(image,name,text,dateCreated) VALUES(?1,'Exposure',?2,1)",
                rusqlite::params![p.id, develop_xmp(p.exposure, p.saturation, "")],
            )
            .map_err(err)?;
        }
        renders.push((
            p.id,
            p.present,
            simulated_render(&src, p.exposure, p.saturation),
        ));
    }
    // Virtual copy of 30: black and white.
    c.execute(
        "INSERT INTO Adobe_images VALUES(?1,20,30,'Black & White','AB','2026-06-10T14:00:00',0,2,'Blue')",
        [COPY],
    )
    .map_err(err)?;
    c.execute(
        "INSERT INTO Adobe_imageDevelopSettings VALUES(?1,?2,'15.4')",
        rusqlite::params![COPY, develop_xmp(0.5, -100.0, "")],
    )
    .map_err(err)?;
    renders.push((
        COPY,
        true,
        simulated_render(&original(0.05, 900, 600), 0.5, -100.0),
    ));

    for (n, (id, present, render)) in renders.into_iter().enumerate() {
        if !present {
            continue; // Lightroom never built a preview for this one.
        }
        let uuid = format!("{:08X}-0000-4000-8000-{:012X}", 0xA0 + n, id);
        let digest = format!("{:032x}", id * 7919);
        let small = image::imageops::thumbnail(&render, 240, 160);
        let large = image::imageops::thumbnail(&render, 720, 480);
        let sections = [
            Section {
                name: "header".into(),
                version: 0,
                kind: 0,
                data: b"levels = { { height = 160, width = 240 }, { height = 480, width = 720 } }"
                    .to_vec(),
            },
            Section {
                name: "level_1".into(),
                version: 0,
                kind: 1,
                data: jpeg(&small)?,
            },
            Section {
                name: "level_2".into(),
                version: 0,
                kind: 1,
                data: jpeg(&large)?,
            },
        ];
        let dir = previews.join(&uuid[..1]).join(&uuid[..4]);
        std::fs::create_dir_all(&dir).map_err(|e| EngineError::io_at(&dir, &e))?;
        let file = dir.join(format!("{uuid}-{digest}.lrprev"));
        std::fs::write(&file, write_lrprev(&sections))
            .map_err(|e| EngineError::io_at(&file, &e))?;
        pc.execute(
            "INSERT INTO ImageCacheEntry(imageId,uuid,digest,orientation) VALUES(?1,?2,?3,'AB')",
            rusqlite::params![id, uuid, digest],
        )
        .map_err(err)?;
    }
    Ok(Fixture {
        catalog,
        photos,
        previews,
    })
}
