//! Synthetic Lightroom catalog of any size, generated in-test (B5-29c). Shapes
//! follow LrC 15.5 (Lua develop rows, BLOB history text, unnamed keyword root)
//! with invented values; nothing is copied from a real catalog.
#![allow(dead_code)]
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

pub const GLOBAL: &str = include_str!("../data/lrc155/global.lua");
pub const STRUCTURES: &str = include_str!("../data/lrc155/structures.lua");

/// What kind of develop row image `i` gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Empty,
    Xmp,
    Lua,
    LuaUnknown,
}

pub fn row_kind(i: i64) -> Row {
    match i % 10 {
        0 => Row::Empty,
        1 | 2 => Row::Xmp,
        3 => Row::LuaUnknown,
        _ => Row::Lua,
    }
}

fn xmp(exposure: f64) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" crs:Exposure2012="{exposure:.2}" crs:Contrast2012="12" crs:FutureKnob="7"><crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 140</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012></rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

fn lua(i: i64, kind: Row) -> String {
    let exposure = format!("Exposure2012 = {:.2}", (i % 400) as f64 / 100.0 - 2.0);
    match kind {
        Row::LuaUnknown => STRUCTURES.replacen("Exposure2012 = -0.5", &exposure, 1),
        _ => GLOBAL.replacen("Exposure2012 = 0.35", &exposure, 1),
    }
}

/// Write an `n`-image catalog at `dir/Synthetic.lrcat` and return its path.
pub fn write(dir: &Path, n: i64) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("Synthetic.lrcat");
    let _ = std::fs::remove_file(&path);
    let mut c = Connection::open(&path).unwrap();
    c.execute_batch(
        r#"
CREATE TABLE Adobe_variablesTable(id_local INTEGER PRIMARY KEY, name TEXT, value TEXT);
INSERT INTO Adobe_variablesTable(name, value) VALUES('Adobe_DBVersion','1504001');
CREATE TABLE AgLibraryRootFolder(id_local INTEGER PRIMARY KEY, absolutePath TEXT, name TEXT);
INSERT INTO AgLibraryRootFolder VALUES(1,'/Volumes/Synthetic/Photos/','Photos');
CREATE TABLE AgLibraryFolder(id_local INTEGER PRIMARY KEY, rootFolder INTEGER, pathFromRoot TEXT);
CREATE TABLE AgLibraryFile(id_local INTEGER PRIMARY KEY, folder INTEGER, baseName TEXT, extension TEXT, md5 TEXT);
CREATE TABLE Adobe_images(id_local INTEGER PRIMARY KEY, rootFile INTEGER, masterImage INTEGER, copyName TEXT, orientation TEXT, captureTime TEXT, pick INTEGER, rating INTEGER, colorLabels TEXT, propertiesCache TEXT);
CREATE TABLE Adobe_imageDevelopSettings(id_local INTEGER PRIMARY KEY, image INTEGER, text TEXT, processVersion TEXT, digest TEXT, hasMasks INTEGER);
CREATE TABLE AgLibraryKeyword(id_local INTEGER PRIMARY KEY, name TEXT, parent INTEGER);
CREATE TABLE AgLibraryKeywordSynonym(keyword INTEGER, name TEXT);
CREATE TABLE AgLibraryKeywordImage(id_local INTEGER PRIMARY KEY, image INTEGER, tag INTEGER);
CREATE TABLE AgLibraryCollection(id_local INTEGER PRIMARY KEY, name TEXT, parent INTEGER, creationId TEXT);
CREATE TABLE AgLibraryCollectionImage(id_local INTEGER PRIMARY KEY, collection INTEGER, image INTEGER, position REAL);
CREATE TABLE AgLibraryCollectionContent(collection INTEGER, content TEXT);
CREATE TABLE Adobe_libraryImageDevelopHistoryStep(id_local INTEGER PRIMARY KEY, image INTEGER, name TEXT, text BLOB, dateCreated REAL, valueString TEXT);
CREATE TABLE Adobe_libraryImageDevelopSnapshot(id_local INTEGER PRIMARY KEY, image INTEGER, name TEXT, text TEXT);
CREATE TABLE AgLibraryFolderStack(id_local INTEGER PRIMARY KEY, folder INTEGER);
CREATE TABLE AgLibraryFolderStackImage(id_local INTEGER PRIMARY KEY, stack INTEGER, image INTEGER, position INTEGER);
CREATE TABLE AgLibraryFace(id_local INTEGER PRIMARY KEY, image INTEGER, cluster INTEGER, tl_x REAL, tl_y REAL, br_x REAL, br_y REAL);
CREATE TABLE AgLibraryFaceCluster(id_local INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE AgLibraryKeywordFace(id_local INTEGER PRIMARY KEY, face INTEGER, keyword INTEGER);
CREATE TABLE AgHarvestedExifMetadata(id_local INTEGER PRIMARY KEY, image INTEGER, gpsLatitude REAL, gpsLongitude REAL, isoSpeedRating REAL);
INSERT INTO AgLibraryKeyword VALUES(1,NULL,NULL);
INSERT INTO AgLibraryKeyword VALUES(2,'Places',1),(3,'People',1),(4,'Alice',3),(5,'Bob',3),(6,'NYC',2),(7,'Paris',2);
INSERT INTO AgLibraryKeywordSynonym VALUES(6,'New York');
INSERT INTO AgLibraryCollection VALUES
 (1,'Sets',NULL,'com.adobe.ag.library.group'),
 (2,'Selects',1,'com.adobe.ag.library.collection'),
 (3,'Review',NULL,'com.adobe.ag.library.collection'),
 (4,'Rated',1,'com.adobe.ag.library.smart_collection');
INSERT INTO AgLibraryCollectionContent VALUES
 (4,'s = { combine = "intersect", { criteria = "rating", operation = ">=", value = 3 } }');
INSERT INTO AgLibraryFaceCluster VALUES(1,'Alice'),(2,NULL);
"#,
    )
    .unwrap();
    let t = c.transaction().unwrap();
    for f in 0..20 {
        t.execute(
            "INSERT INTO AgLibraryFolder VALUES(?1,1,?2)",
            params![100 + f, format!("2026/shoot-{f:02}/")],
        )
        .unwrap();
    }
    let (mut history, mut faces, mut stack) = (1i64, 1i64, 1i64);
    for i in 1..=n {
        let id = 1000 + i;
        // Every 50th image is a virtual copy of the previous one.
        let copy = i % 50 == 0;
        if !copy {
            t.execute(
                "INSERT INTO AgLibraryFile VALUES(?1,?2,?3,'NEF',?4)",
                params![id, 100 + i % 20, format!("IMG_{i:05}"), format!("{i:032x}")],
            )
            .unwrap();
        }
        t.execute(
            "INSERT INTO Adobe_images VALUES(?1,?2,?3,?4,'AB',?5,?6,?7,?8,?9)",
            params![
                id,
                (!copy).then_some(id),
                copy.then_some(id - 1),
                copy.then_some("Copy 1"),
                format!("2026-01-{:02}T10:{:02}:00", 1 + i % 28, i % 60),
                [0, 1, -1][(i % 3) as usize],
                i % 6,
                ["", "Red", "Blue"][(i % 3) as usize],
                "{ cached = true }",
            ],
        )
        .unwrap();
        let kind = row_kind(i);
        let (text, pv) = match kind {
            Row::Empty => (None, None),
            Row::Xmp => (Some(xmp((i % 300) as f64 / 100.0 - 1.5)), Some("15.4")),
            Row::Lua | Row::LuaUnknown => (Some(lua(i, kind)), Some("15.4")),
        };
        t.execute(
            "INSERT INTO Adobe_imageDevelopSettings(image,text,processVersion,digest,hasMasks) VALUES(?1,?2,?3,?4,0)",
            params![id, text, pv, format!("{i:016x}")],
        )
        .unwrap();
        if let Some(text) = &text {
            for step in 0..(1 + i % 3) {
                t.execute(
                    "INSERT INTO Adobe_libraryImageDevelopHistoryStep VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        history,
                        id,
                        format!("Step {step}"),
                        text.as_bytes(),
                        800_000_000.0 + (history as f64),
                        (step == 0).then_some("0.35"),
                    ],
                )
                .unwrap();
                history += 1;
            }
        }
        if i % 3 == 0 {
            t.execute(
                "INSERT INTO AgLibraryFace VALUES(?1,?2,?3,0.1,0.2,0.3,0.4)",
                params![faces, id, 1 + faces % 2],
            )
            .unwrap();
            if faces % 4 == 0 {
                t.execute(
                    "INSERT INTO AgLibraryKeywordFace(face,keyword) VALUES(?1,4)",
                    params![faces],
                )
                .unwrap();
            }
            faces += 1;
        }
        if i % 2 == 0 {
            t.execute(
                "INSERT INTO AgHarvestedExifMetadata(image,gpsLatitude,gpsLongitude,isoSpeedRating) VALUES(?1,?2,?3,100)",
                params![id, 40.0 + (i % 100) as f64 / 100.0, -74.0],
            )
            .unwrap();
        }
        if i % 4 == 0 {
            t.execute(
                "INSERT INTO AgLibraryKeywordImage(image,tag) VALUES(?1,?2)",
                params![id, 4 + i % 4],
            )
            .unwrap();
        }
        if i % 7 == 0 {
            // Positions in reverse, so membership order is not rowid order.
            t.execute(
                "INSERT INTO AgLibraryCollectionImage(collection,image,position) VALUES(?1,?2,?3)",
                params![2 + i % 2, id, (n - i) as f64],
            )
            .unwrap();
        }
        if i % 100 == 1 && i + 1 <= n {
            t.execute(
                "INSERT INTO AgLibraryFolderStack VALUES(?1,?2)",
                params![stack, 100 + i % 20],
            )
            .unwrap();
            t.execute(
                "INSERT INTO AgLibraryFolderStackImage(stack,image,position) VALUES(?1,?2,2),(?1,?3,1)",
                params![stack, id, id + 1],
            )
            .unwrap();
            stack += 1;
        }
    }
    t.commit().unwrap();
    path
}
