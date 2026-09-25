use super::*;
struct CancelOnWrite {
    inner: std::io::Cursor<Vec<u8>>,
    cancel: CancellationToken,
}
impl Write for CancelOnWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.cancel.cancel();
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Seek for CancelOnWrite {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}
#[test]
fn cancel_during_encoding_stops_writes() {
    for format in [
        Format::Jpeg { quality: 90 },
        Format::Png,
        Format::Tiff { bits: 16 },
    ] {
        let cancel = CancellationToken::new();
        let mut writer = CancelOnWrite {
            inner: std::io::Cursor::new(Vec::new()),
            cancel: cancel.clone(),
        };
        let image = image::Rgb32FImage::from_pixel(64, 64, image::Rgb([0.2, 0.3, 0.4]));
        assert_eq!(
            encode(&mut writer, &image, format, ColorSpace::Srgb, None, &cancel),
            Err(engine_api::EngineError::Cancelled)
        );
        assert!(writer.inner.get_ref().len() < 100);
    }
}
