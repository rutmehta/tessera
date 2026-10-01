//! Counts-only FFI acceptance driver. All roots are relocated into an empty
//! temporary directory so measurement cannot write beside source originals.
use std::time::Instant;
use tessera_ffi::{Engine, inspect_lrcat};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("inspect or apply");
    let path = args.next().expect("catalog copy");
    let start = Instant::now();
    if mode == "inspect" {
        let s = inspect_lrcat(path)?;
        println!(
            "images={} edited={} copies={} inspect_seconds={:.3}",
            s.images,
            s.edited,
            s.virtual_copies,
            start.elapsed().as_secs_f64()
        );
    } else {
        let temp = tempfile::tempdir()?;
        let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned())?;
        let import = engine.open_lrcat(path)?;
        let opened = start.elapsed();
        let mut options = import.default_options();
        options.library_folder = temp.path().join("library").to_string_lossy().into_owned();
        for (i, r) in options.relocations.iter_mut().enumerate() {
            r.to = temp
                .path()
                .join(format!("absent-{i}"))
                .to_string_lossy()
                .into_owned();
        }
        let apply = Instant::now();
        let r = import.apply(options, None)?;
        println!(
            "images={} imported={} skipped={} copies={} open_seconds={:.3} apply_seconds={:.3} total_seconds={:.3}",
            import.summary().images,
            r.imported,
            r.skipped.len(),
            r.virtual_copies,
            opened.as_secs_f64(),
            apply.elapsed().as_secs_f64(),
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
