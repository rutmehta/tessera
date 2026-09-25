use lens::*;
fn warped(p: Point) -> Point {
    let d = 1. + 0.14 * p[0] + 0.19 * p[1];
    [(p[0] + 0.07 * p[1]) / d, (-0.04 * p[0] + p[1]) / d]
}
#[test]
fn robust_full_upright_and_guides() {
    let mut lines = Vec::new();
    for t in [-0.8, -0.4, 0., 0.4, 0.8] {
        for (a, b) in [([t, -0.9], [t, 0.9]), ([-0.9, t], [0.9, t])] {
            lines.push(LineSegment {
                start: warped(a),
                end: warped(b),
                points: vec![],
                strength: 1.,
            });
        }
    }
    lines.push(LineSegment {
        start: [-0.6, -0.7],
        end: [-0.2, 0.9],
        points: vec![],
        strength: 1.,
    });
    let result = estimate_upright(&lines, UprightMode::Full).unwrap();
    for (i, l) in lines[..10].iter().enumerate() {
        let a = result.homography.map(l.start).unwrap();
        let b = result.homography.map(l.end).unwrap();
        assert!((a[i % 2] - b[i % 2]).abs() < 1e-6, "{a:?} {b:?}");
    }
    let guides = vec![
        Guide {
            start: lines[0].start,
            end: lines[0].end,
            axis: GuideAxis::Vertical,
        },
        Guide {
            start: lines[8].start,
            end: lines[8].end,
            axis: GuideAxis::Vertical,
        },
    ];
    let g = guided_upright(&guides).unwrap();
    for l in &guides {
        let a = g.homography.map(l.start).unwrap();
        let b = g.homography.map(l.end).unwrap();
        assert!((a[0] - b[0]).abs() < 1e-8);
    }
    assert!(estimate_upright(&[], UprightMode::Full).is_none());
    assert!(guided_upright(&[]).is_none());
}
