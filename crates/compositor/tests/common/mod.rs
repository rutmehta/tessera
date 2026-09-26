#![allow(dead_code)]
use compositor::*;
use engine_api::tile::Extent;

/// A pixel layer whose pixel (x, y) is `f(x, y)` (straight RGBA).
pub fn layer_fn(name: &str, e: Extent, depth: Depth, f: impl Fn(u32, u32) -> [f32; 4]) -> Layer {
    let mut l = Layer::pixel(name, e, depth);
    if let Some(r) = l.raster_mut() {
        r.edit_region(Rect::of_extent(e), 1, |x, y, p| *p = f(x, y))
            .unwrap();
    }
    l
}

/// A pixel layer with the given row-0 pixels.
pub fn layer_px(name: &str, px: &[[f32; 4]]) -> Layer {
    let e = Extent::new(px.len() as u32, 1);
    let px = px.to_vec();
    layer_fn(name, e, Depth::F32, move |x, _| px[x as usize])
}

pub fn opaque(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

pub fn doc(e: Extent, depth: Depth) -> Document {
    Document::new(DocState::new(e, depth))
}

/// Adds a layer on top of `parent` and returns its id.
pub fn add(doc: &mut Document, parent: Option<LayerId>, layer: Layer) -> LayerId {
    doc.apply(DocOp::AddLayer {
        parent,
        index: usize::MAX,
        layer,
    })
    .unwrap()
    .created[0]
}

/// Level-0 composite as interleaved straight RGBA.
pub fn render(doc: &Document) -> Vec<f32> {
    Compositor::new(256 << 20)
        .render_level_rgba(doc, 0)
        .unwrap()
        .1
}

pub fn px(img: &[f32], w: u32, x: u32, y: u32) -> [f32; 4] {
    let i = ((y * w + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

#[track_caller]
pub fn assert_close(got: &[f32], want: &[f32], tol: f32, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w).abs() <= tol,
            "{what}: [{i}] got {g}, want {w} (tol {tol})\n got {got:?}\nwant {want:?}"
        );
    }
}

pub fn set_props(doc: &mut Document, id: LayerId, f: impl FnOnce(&mut LayerProps)) {
    let mut p = doc.state().find(id).unwrap().props.clone();
    f(&mut p);
    doc.apply(DocOp::SetProps { id, props: p }).unwrap();
}
