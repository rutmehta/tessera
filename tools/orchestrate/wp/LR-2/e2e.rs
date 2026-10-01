// Synthetic SQLite catalog -> Lua develop -> recipe -> CPU operators.
use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let f = import_lrcat::fixture::write(dir.path()).unwrap();
    let c = rusqlite::Connection::open(&f.catalog).unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='5.7' WHERE image=30", ["s = { Exposure=1, ConvertToGrayscale=true, GrayMixerRed=50, GrayMixerOrange=50, GrayMixerYellow=50, GrayMixerGreen=50, GrayMixerAqua=50, GrayMixerBlue=50, GrayMixerPurple=50, GrayMixerMagenta=50, ExtendedToneCurvePV2012={0,0,255,255} }"]).unwrap();
    drop(c);
    let plan = import_lrcat::import(&f.catalog).unwrap();
    let r = &plan.images.iter().find(|i| i.catalog_id==30).unwrap().recipe;
    r.validate().unwrap();
    for key in ["ConvertToGrayscale", "GrayMixerRed", "ExtendedToneCurvePV2012"] {
        assert!(r.unknown["lrcat_develop_source"]["properties"][key].is_null());
    }
    let mut max_error=0f32;
    for (rgb, expected) in [([1.,0.,0.],0.7881),([0.,1.,0.],2.034),([0.,0.,1.],0.1779),([0.18;3],0.36)] {
        let mut tile=Tile::from_samples(TileCoord::new(0,0,0),TileLayout{extent:Extent::new(1,1),halo:0,channels:3},rgb.to_vec()).unwrap();
        pipeline_cpu::tone(&mut tile,&r.settings.tone).unwrap();
        pipeline_cpu::tone_extra(&mut tile,&r.settings.tone).unwrap();
        pipeline_cpu::color(&mut tile,&r.settings.color).unwrap();
        for value in tile.samples::<f32>().unwrap() { max_error=max_error.max((value-expected).abs()); }
    }
    println!("synthetic import: 4 swatches / 12 channels, maximum absolute error={max_error:.9}, tolerance=0.000002");
    assert!(max_error<2e-6);
}
