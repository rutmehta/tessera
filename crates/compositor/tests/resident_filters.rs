use compositor::document::SmartFilter;
use compositor::geom::Rect;
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;

fn document(filters: Vec<SmartFilter>) -> Document {
    let e = Extent::new(17, 13);
    let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            *p = [x as f32 / 19.0, y as f32 / 17.0, 0.25, 0.5];
        })
        .unwrap();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("pixels", LayerKind::Pixel(raster))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters = filters;
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("filtered", LayerKind::SmartObject(so))));
    Document::new(state)
}

#[test]
fn resident_and_fallback_receive_native_child_context() {
    use compositor::render::smart_filters::{
        FilterContext, ResidentFilterEvaluator, SmartFilterEvaluator,
    };
    use std::sync::Mutex;
    struct Capture {
        resident: bool,
        seen: Mutex<Vec<FilterContext>>,
    }
    impl SmartFilterEvaluator for Capture {
        fn evaluate(
            &self,
            input: &Raster,
            _: &SmartFilter,
            context: &FilterContext,
        ) -> engine_api::EngineResult<Raster> {
            assert!(!self.resident);
            self.seen.lock().unwrap().push(context.clone());
            Ok(input.clone())
        }
    }
    impl ResidentFilterEvaluator for Capture {
        fn supports(&self, _: &SmartFilter) -> engine_api::EngineResult<bool> {
            Ok(self.resident)
        }
        fn evaluate_resident(
            &self,
            _: &wgpu::Device,
            _: &wgpu::Queue,
            input: &wgpu::Buffer,
            extent: Extent,
            _: &SmartFilter,
            context: &FilterContext,
        ) -> engine_api::EngineResult<wgpu::Buffer> {
            assert!(self.resident);
            assert_eq!(extent, context.canvas);
            self.seen.lock().unwrap().push(context.clone());
            Ok(input.clone())
        }
    }
    let gpu = GpuCompositor::new().unwrap();
    for resident_route in [true, false] {
        let mut resident = ResidentRenderer::new(&gpu).unwrap();
        let capture = Arc::new(Capture {
            resident: resident_route,
            seen: Mutex::new(Vec::new()),
        });
        resident.set_filter_evaluator(capture.clone()).unwrap();
        let doc = document(vec![SmartFilter {
            name: "capture".into(),
            enabled: true,
            ..Default::default()
        }]);
        let mut state = (**doc.state()).clone();
        state.profile = Some(compositor::ColorProfile::from_icc("outer", vec![9]));
        let LayerKind::SmartObject(so) = &mut Arc::make_mut(&mut state.root[0]).kind else {
            unreachable!()
        };
        Arc::make_mut(&mut so.state).profile =
            Some(compositor::ColorProfile::from_icc("child", vec![1, 2, 3]));
        let doc = Document::new(state);
        resident.render(&doc, 2).unwrap();
        resident.render(&doc, 0).unwrap();
        let seen = capture.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].level, 0);
        assert_eq!(seen[0].canvas, Extent::new(17, 13));
        assert_eq!(
            seen[0].profile.as_ref().unwrap().icc.as_deref().unwrap(),
            &vec![1, 2, 3]
        );
        drop(seen);
        // Preserve source identity/revision: a profile-only change must not
        // reuse a resident stage or CPU fallback result from the old context.
        let mut state = (**doc.state()).clone();
        let LayerKind::SmartObject(so) = &mut Arc::make_mut(&mut state.root[0]).kind else {
            unreachable!()
        };
        Arc::make_mut(&mut so.state).profile = None;
        resident.render(&Document::new(state), 2).unwrap();
        let seen = capture.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].profile.is_none());
    }
}

#[test]
fn resident_invert_is_not_silently_omitted() {
    let gpu = GpuCompositor::new().unwrap();
    let doc = document(vec![SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    }]);
    let mut resident = ResidentRenderer::new(&gpu).unwrap();
    let cpu = Compositor::new(1 << 20);
    for level in [0, 2] {
        resident.render(&doc, level).unwrap();
        let actual = resident.read_level(level, false).unwrap().1;
        let expected = cpu.render_level_rgba(&doc, level).unwrap().1;
        assert_eq!(actual.len(), expected.len());
        let error = actual
            .iter()
            .zip(expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(error <= 1e-4, "L{level}: {error}");
    }
}
