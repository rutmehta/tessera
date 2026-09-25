use rusqlite::Connection;
use std::path::Path;

pub fn make_fixture(path: &Path) -> Connection {
    let c = Connection::open(path).unwrap();
    c.execute_batch(r#"
CREATE TABLE Adobe_variablesTable(name TEXT, value TEXT);
INSERT INTO Adobe_variablesTable VALUES('Adobe_DBVersion','1300000');
CREATE TABLE AgLibraryRootFolder(id_local INTEGER, absolutePath TEXT);
INSERT INTO AgLibraryRootFolder VALUES(1,'/photos/');
CREATE TABLE AgLibraryFolder(id_local INTEGER, rootFolder INTEGER, pathFromRoot TEXT);
INSERT INTO AgLibraryFolder VALUES(10,1,'a/'),(11,1,'b/');
CREATE TABLE AgLibraryFile(id_local INTEGER, folder INTEGER, baseName TEXT, extension TEXT);
INSERT INTO AgLibraryFile VALUES(20,10,'one','CR3'),(21,11,'two','DNG');
CREATE TABLE Adobe_images(id_local INTEGER, rootFile INTEGER, masterImage INTEGER, copyName TEXT, orientation TEXT, captureTime TEXT, pick INTEGER, rating INTEGER, colorLabels TEXT);
INSERT INTO Adobe_images VALUES(30,20,NULL,NULL,'AB','2026-01-01',1,5,'Client'),(31,21,NULL,NULL,'BC','2026-01-02',-1,3,''),(32,20,30,'Black & White','AB','2026-01-01',0,2,'Blue');
CREATE TABLE Adobe_imageDevelopSettings(image INTEGER, text TEXT, processVersion TEXT);
CREATE TABLE AgLibraryKeyword(id_local INTEGER, name TEXT, parent INTEGER);
INSERT INTO AgLibraryKeyword VALUES(1,'Places',NULL),(2,'NYC',1),(3,'Alice',NULL);
CREATE TABLE AgLibraryKeywordImage(image INTEGER, tag INTEGER);
INSERT INTO AgLibraryKeywordImage VALUES(30,2);
CREATE TABLE AgLibraryKeywordSynonym(keyword INTEGER, name TEXT);
INSERT INTO AgLibraryKeywordSynonym VALUES(2,'New York');
CREATE TABLE AgLibraryCollection(id_local INTEGER, name TEXT, parent INTEGER, creationId TEXT);
INSERT INTO AgLibraryCollection VALUES(1,'Portfolio',NULL,'com.adobe.ag.library.collection_set'),(2,'Selects',1,'com.adobe.ag.library.collection'),(3,'Best',1,'com.adobe.ag.library.smart_collection');
CREATE TABLE AgLibraryCollectionImage(collection INTEGER, image INTEGER, position REAL);
INSERT INTO AgLibraryCollectionImage VALUES(2,30,1),(2,32,2);
CREATE TABLE AgLibraryCollectionContent(collection INTEGER, content TEXT);
INSERT INTO AgLibraryCollectionContent VALUES(3,'{ combine = "intersect", { criteria = "rating", operation = ">=", value = 3 } }');
CREATE TABLE Adobe_libraryImageDevelopHistoryStep(id_local INTEGER, image INTEGER, name TEXT, text TEXT, dateCreated REAL);
CREATE TABLE Adobe_libraryImageDevelopSnapshot(id_local INTEGER, image INTEGER, name TEXT, text TEXT);
CREATE TABLE AgLibraryFolderStack(id_local INTEGER, folder INTEGER);
INSERT INTO AgLibraryFolderStack VALUES(1,10);
CREATE TABLE AgLibraryFolderStackImage(stack INTEGER, image INTEGER, position INTEGER);
INSERT INTO AgLibraryFolderStackImage VALUES(1,30,1),(1,32,2);
CREATE TABLE AgLibraryFace(id_local INTEGER, image INTEGER, cluster INTEGER, x REAL, y REAL, width REAL, height REAL);
INSERT INTO AgLibraryFace VALUES(1,30,1,0.1,0.2,0.3,0.4);
CREATE TABLE AgLibraryFaceCluster(id_local INTEGER, name TEXT);
INSERT INTO AgLibraryFaceCluster VALUES(1,'Alice');
CREATE TABLE AgLibraryKeywordFace(face INTEGER, keyword INTEGER);
INSERT INTO AgLibraryKeywordFace VALUES(1,3);
CREATE TABLE AgHarvestedExifMetadata(image INTEGER, gpsLatitude REAL, gpsLongitude REAL);
INSERT INTO AgHarvestedExifMetadata VALUES(30,40.7,-74.0);
"#).unwrap();
    let xmp = r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+0.5" crs:Contrast2012="10" crs:FutureKnob="42"><crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 140</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li crs:CorrectionName="Sky" crs:LocalExposure2012="0.2"><crs:CorrectionMasks><rdf:Seq><rdf:li crs:What="Mask/Sky"/></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description>"#;
    c.execute(
        "INSERT INTO Adobe_imageDevelopSettings VALUES(30,?1,'15.4')",
        [xmp],
    )
    .unwrap();
    c.execute(
        "INSERT INTO Adobe_libraryImageDevelopHistoryStep VALUES(1,30,'Exposure',?1,1)",
        [xmp],
    )
    .unwrap();
    c.execute(
        "INSERT INTO Adobe_libraryImageDevelopSnapshot VALUES(1,30,'First',?1)",
        [xmp],
    )
    .unwrap();
    c
}
