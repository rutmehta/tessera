// Synthetic 16x16 RGB animation; libjxl 0.11.1, no photographic input.
// cc -I/opt/homebrew/include -L/opt/homebrew/lib generate-two-frame.c -ljxl -o /tmp/gen
// /tmp/gen > two-frame.jxl
#include <jxl/encode.h>
#include <assert.h>
#include <stdio.h>
int main(void) {
  JxlEncoder *enc = JxlEncoderCreate(NULL);
  JxlBasicInfo info;
  JxlEncoderInitBasicInfo(&info);
  info.xsize = info.ysize = 16;
  info.bits_per_sample = 8;
  info.num_color_channels = 3;
  info.uses_original_profile = JXL_TRUE;
  info.have_animation = JXL_TRUE;
  info.animation.tps_numerator = 10;
  info.animation.tps_denominator = 1;
  assert(JxlEncoderSetBasicInfo(enc, &info) == JXL_ENC_SUCCESS);
  JxlColorEncoding color;
  JxlColorEncodingSetToSRGB(&color, JXL_FALSE);
  assert(JxlEncoderSetColorEncoding(enc, &color) == JXL_ENC_SUCCESS);
  JxlEncoderFrameSettings *settings = JxlEncoderFrameSettingsCreate(enc, NULL);
  assert(JxlEncoderSetFrameLossless(settings, JXL_TRUE) == JXL_ENC_SUCCESS);
  JxlFrameHeader header;
  JxlEncoderInitFrameHeader(&header);
  header.duration = 1;
  assert(JxlEncoderSetFrameHeader(settings, &header) == JXL_ENC_SUCCESS);
  JxlPixelFormat format = {3, JXL_TYPE_UINT8, JXL_NATIVE_ENDIAN, 0};
  unsigned char pixels[16 * 16 * 3];
  for (int f = 0; f < 2; ++f) {
    for (int i = 0; i < sizeof(pixels); ++i) pixels[i] = 48 + f * 32 + i % 3;
    assert(JxlEncoderAddImageFrame(settings, &format, pixels, sizeof(pixels)) == JXL_ENC_SUCCESS);
  }
  JxlEncoderCloseInput(enc);
  unsigned char output[65536], *next = output;
  size_t available = sizeof(output);
  assert(JxlEncoderProcessOutput(enc, &next, &available) == JXL_ENC_SUCCESS);
  fwrite(output, 1, next - output, stdout);
  JxlEncoderDestroy(enc);
}
