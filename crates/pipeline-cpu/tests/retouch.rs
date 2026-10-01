//! LR-3b: supported heal/clone recipes must reach the Develop CPU renderer.
use engine_api::{
    id::RetouchId,
    recipe::{
        DevelopSettings, MaskComponent, MaskKind,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
    },
};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

#[test]
fn heal_and_clone_spots_render_through_develop_cpu() {
    let plane: Vec<f32> = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 0.8 } else { 0.1 }))
        .collect();
    let image = Image::new(128, 80, vec![plane; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    let baseline = render_linear_scaled(&settings, &RenderSource::Rgb(&image), 1).unwrap();
    settings.locals.retouch = [
        RetouchKind::Clone {
            source_offset: [0.5, 0.0],
        },
        RetouchKind::Heal {
            source_offset: [0.5, 0.0],
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| RetouchOperation {
        id: RetouchId(i as u32),
        kind,
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.3 + i as f32 * 0.4, 1.0]],
                    radius: 0.0625,
                    feather: 50.0,
                    ..BrushStroke::default()
                }],
            })],
        },
        opacity: 50.0,
        feather: 0.0,
        enabled: true,
    })
    .collect();
    let rendered = render_linear_scaled(&settings, &RenderSource::Rgb(&image), 1)
        .expect("Develop CPU must render supported heal and clone spots");
    assert_eq!((rendered.width(), rendered.height()), (128, 80));
    // A successful return that silently drops retouch must also fail this test.
    assert!(rendered.planes()[0][24 * 128 + 32] > baseline.planes()[0][24 * 128 + 32] + 0.1);
}
