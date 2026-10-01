//! A separate test process isolates SQLite's allocation high-water mark.
//! This catches loading a whole oversized SQLite value even without a Rust copy.
use rusqlite::{Connection, ffi};

fn largest_sqlite_allocation(reset: bool) -> i64 {
    let (mut current, mut peak) = (0, 0);
    // SAFETY: both out-pointers are valid and SQLite serializes its status counters.
    let rc = unsafe {
        ffi::sqlite3_status64(
            ffi::SQLITE_STATUS_MALLOC_SIZE,
            &mut current,
            &mut peak,
            i32::from(reset),
        )
    };
    assert_eq!(rc, ffi::SQLITE_OK);
    peak
}

#[test]
fn oversized_sqlite_cells_are_streamed_without_full_cell_allocations() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fx")).unwrap();
    let c = Connection::open(&fixture.catalog).unwrap();
    let length = 32 * 1024 * 1024;
    c.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=CAST(zeroblob(?1) AS TEXT) WHERE image=30",
        [length],
    )
    .unwrap();
    drop(c);
    for externalize in [false, true] {
        let bundle = temp.path().join("bundle");
        largest_sqlite_allocation(true);
        let mut cell = None;
        import_lrcat::import_each_with_storage(
            &fixture.catalog,
            externalize.then_some(bundle.as_path()),
            |_| Ok(()),
            |image| {
                if image.catalog_id == 30 {
                    cell = Some(image.recipe.unknown["lrcat_develop_source"]["cell"].clone());
                }
                Ok(())
            },
        )
        .unwrap();
        let peak = largest_sqlite_allocation(false);
        assert!(
            peak < import_lrcat::MAX_CELL_BYTES as i64,
            "SQLite allocated {peak} bytes for a {length}-byte cell (externalize={externalize})"
        );
        let cell = cell.unwrap();
        assert_eq!(cell["length"], length);
        if externalize {
            use std::io::Read;
            let mut file =
                std::fs::File::open(bundle.join(cell["path"].as_str().unwrap())).unwrap();
            let mut buffer = [0; 64 * 1024];
            let mut total = 0;
            loop {
                let n = file.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                assert!(buffer[..n].iter().all(|v| *v == 0));
                total += n;
            }
            assert_eq!(total, length as usize);
        }
    }
}
