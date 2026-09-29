// Test-only native reader, distinct from the ImageIO CGImage acceptance path.
#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
#import <CoreGraphics/CoreGraphics.h>
#include <math.h>

static NSDictionary *readPixels(CIContext *context, CIImage *image, CGColorSpaceRef space,
                                NSString *output) {
    if (!image || image.extent.size.width != 80 || image.extent.size.height != 16)
        return @{@"valid": @NO};
    float pixels[80 * 16 * 4] = {0};
    [context render:image toBitmap:pixels rowBytes:80 * 16 bounds:image.extent
             format:kCIFormatRGBAf colorSpace:space];
    BOOL valid = YES;
    float peak = 0;
    for (size_t i = 0; i < 80 * 16; ++i) {
        valid &= pixels[4 * i + 3] > 0.99f;
        for (size_t c = 0; c < 4; ++c) valid &= isfinite(pixels[4 * i + c]);
        for (size_t c = 0; c < 3; ++c) peak = fmaxf(peak, pixels[4 * i + c]);
    }
    BOOL written = [[NSData dataWithBytes:pixels length:sizeof(pixels)] writeToFile:output atomically:YES];
    return @{@"valid": (valid && written) ? @YES : @NO, @"headroom": @(image.contentHeadroom), @"peak": @(peak)};
}
int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc != 4) return 2;
        NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[1]]];
        NSString *directory = [NSString stringWithUTF8String:argv[2]];
        float requested = strtof(argv[3], NULL);
        CGColorSpaceRef linear = CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearSRGB);
        CIContext *context = [CIContext contextWithOptions:@{
            kCIContextUseSoftwareRenderer: @YES,
            kCIContextWorkingColorSpace: (__bridge id)linear,
            kCIContextOutputColorSpace: (__bridge id)linear,
            kCIContextWorkingFormat: @(kCIFormatRGBAf)}];
        CIImage *base = [CIImage imageWithContentsOfURL:url options:@{kCIImageExpandToHDR: @NO}];
        CIImage *gain = [CIImage imageWithContentsOfURL:url options:@{kCIImageAuxiliaryHDRGainMap: @YES}];
        CIImage *expanded = [CIImage imageWithContentsOfURL:url options:@{kCIImageExpandToHDR: @YES}];
        CIImage *applied = (base && gain) ? [base imageByApplyingGainMap:gain headroom:requested] : base;
        NSDictionary *result = @{
            @"gain_present": gain != nil ? @YES : @NO,
            @"base": readPixels(context, base, linear, [directory stringByAppendingPathComponent:@"base.f32"]),
            @"expanded": readPixels(context, expanded, linear, [directory stringByAppendingPathComponent:@"expanded.f32"]),
            @"explicit": readPixels(context, applied, linear, [directory stringByAppendingPathComponent:@"explicit.f32"])};
        NSData *json = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
        if (!json) return 3;
        fwrite(json.bytes, 1, json.length, stdout);
        CGColorSpaceRelease(linear);
    }
}
