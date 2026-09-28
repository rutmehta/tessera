#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
#import <CoreGraphics/CoreGraphics.h>
#include <math.h>
static NSDictionary *render(CIContext *context, CIImage *image, CGColorSpaceRef space, NSString *name, NSString *directory) {
    if(!image) return @{@"image_created":@NO};
    CGRect extent=image.extent; size_t w=extent.size.width,h=extent.size.height;
    if(w==0||h==0||w*h>16000000) return @{@"invalid_dimensions":@YES};
    float *pixels=calloc(w*h*4,sizeof(float));
    [context render:image toBitmap:pixels rowBytes:w*16 bounds:extent format:kCIFormatRGBAf colorSpace:space];
    double peak=-INFINITY,lo=INFINITY,alphaLo=INFINITY,alphaHi=-INFINITY;size_t nonfinite=0,nonzero=0;
    for(size_t i=0;i<w*h;i++) {alphaLo=fmin(alphaLo,pixels[4*i+3]);alphaHi=fmax(alphaHi,pixels[4*i+3]);for(size_t c=0;c<3;c++){float v=pixels[4*i+c];if(!isfinite(v)){nonfinite++;continue;}lo=fmin(lo,v);peak=fmax(peak,v);nonzero+=v!=0;}}
    NSMutableArray *patches=[NSMutableArray array];
    NSArray *fractions=@[@0.10,@0.30,@0.50,@0.70,@0.90]; size_t y=h/2;
    for(NSNumber *fraction in fractions){size_t x=(size_t)floor(w*fraction.doubleValue);if(x>=w)x=w-1;size_t i=(y*w+x)*4;[patches addObject:@{@"x":@(x),@"y":@(y),@"fraction":fraction,@"rgba":@[@(pixels[i]),@(pixels[i+1]),@(pixels[i+2]),@(pixels[i+3])]}];}
    [[NSData dataWithBytes:pixels length:w*h*16] writeToFile:[directory stringByAppendingPathComponent:[name stringByAppendingString:@".rgba-f32le"]] atomically:YES];free(pixels);
    return @{@"image_created":@YES,@"width":@(w),@"height":@(h),@"content_headroom":@(image.contentHeadroom),@"peak":@(peak),@"min":@(lo),@"nonfinite":@(nonfinite),@"nonzero":@(nonzero),@"alpha_min":@(alphaLo),@"alpha_max":@(alphaHi),@"patches":patches,@"valid_pixels":@(nonfinite==0&&alphaLo>.99&&nonzero>0)};
}
int main(int argc,char **argv){@autoreleasepool {
    if(argc<3)return 2;
    NSString *directory=[NSString stringWithUTF8String:argv[1]];
    CGColorSpaceRef linear=CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearSRGB);
    CIContext *context=[CIContext contextWithOptions:@{kCIContextUseSoftwareRenderer:@YES,kCIContextWorkingColorSpace:(__bridge id)linear,kCIContextOutputColorSpace:(__bridge id)linear,kCIContextWorkingFormat:@(kCIFormatRGBAf)}];
    NSMutableArray *records=[NSMutableArray array];
    for(int i=2;i<argc;i++) {
        NSString *path=[NSString stringWithUTF8String:argv[i]];
        NSURL *url=[NSURL fileURLWithPath:path];
        CIImage *base=[CIImage imageWithContentsOfURL:url options:@{kCIImageExpandToHDR:@NO}];
        CIImage *gain=[CIImage imageWithContentsOfURL:url options:@{kCIImageAuxiliaryHDRGainMap:@YES}];
        CIImage *expanded=[CIImage imageWithContentsOfURL:url options:@{kCIImageExpandToHDR:@YES}];
        float headroom=[path containsString:@"reference-4"]?4:16;
        CIImage *applied=(base&&gain)?[base imageByApplyingGainMap:gain headroom:headroom]:nil;
        NSString *prefix=path.lastPathComponent;
        if(gain) [[gain.properties description] writeToFile:[directory stringByAppendingPathComponent:[prefix stringByAppendingString:@".gain-properties.txt"]] atomically:YES encoding:NSUTF8StringEncoding error:nil];
        [records addObject:@{@"path":path,@"base_created":@(base!=nil),@"gain_created":@(gain!=nil),@"default_expand":render(context,expanded,linear,[prefix stringByAppendingString:@".default"],directory),@"explicit_gain":render(context,applied,linear,[prefix stringByAppendingString:@".explicit"],directory)}];
    }
    NSData *json=[NSJSONSerialization dataWithJSONObject:records options:NSJSONWritingPrettyPrinted error:nil];fwrite(json.bytes,1,json.length,stdout);puts("");CGColorSpaceRelease(linear);
}}
