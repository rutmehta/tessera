use agent::{Agent, Config};
use style_profile::{Profile, Questionnaire};
#[test]
fn jpeg_exif_reaches_perception_packet() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("exif.jpg");
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::from_pixel(
            32,
            32,
            image::Rgb([100, 100, 100]),
        ))
        .unwrap();
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x0f\x01\x02\0\x04\0\0\0CAM\0\0\0\0\0";
    let mut bytes = jpeg[..2].to_vec();
    bytes.extend([0xff, 0xe1]);
    bytes.extend(((exif.len() + 2) as u16).to_be_bytes());
    bytes.extend(exif);
    bytes.extend(&jpeg[2..]);
    std::fs::write(&p, bytes).unwrap();
    let mut a = Agent::open(
        d.path().join("app"),
        Profile::new("test", Questionnaire::default()).unwrap(),
        Config::default(),
    )
    .unwrap();
    let packet = a.perceive(&p).unwrap();
    assert!(
        packet.exif["embedded"]["Make"]
            .as_str()
            .unwrap()
            .contains("CAM")
    );
}
