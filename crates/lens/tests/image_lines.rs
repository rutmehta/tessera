use lens::*;
#[test]
fn detect_edges_not_flat_field() {
    let mut data = vec![0.; 96 * 96];
    for y in 0..96 {
        for x in 0..96 {
            if x > 25 && x < 70 {
                data[y * 96 + x] = 1.;
            }
        }
    }
    let image = GrayImage::new(96, 96, data).unwrap();
    let lines = detect_lines(&image, 0.1, 20);
    assert!(lines.len() >= 2, "{}", lines.len());
    for l in &lines {
        assert!((l.end[0] - l.start[0]).abs() < 0.04);
        assert!(l.points.len() >= 20);
    }
    assert!(detect_lines(&GrayImage::new(32, 32, vec![0.5; 1024]).unwrap(), 0.1, 10).is_empty());
    assert!(GrayImage::new(2, 2, vec![0.; 3]).is_err());
}
