#import <Foundation/Foundation.h>
#import <ImageIO/ImageIO.h>
#import <CoreGraphics/CoreGraphics.h>
#include <math.h>
#import <CommonCrypto/CommonDigest.h>

static NSDictionary *decode(CGImageSourceRef src, CFStringRef request, float **out, size_t *count) {
    NSMutableDictionary *options = [@{(id)kCGImageSourceDecodeRequest:(__bridge id)request} mutableCopy];
    const char *mode = getenv("PROBE_OPTIONS");
    if(!mode || strstr(mode,"float")) options[(id)kCGImageSourceShouldAllowFloat]=@YES;
    if(!mode || strstr(mode,"cache")) options[(id)kCGImageSourceShouldCacheImmediately]=@YES;
    if(getenv("PROBE_LUMA_OFF")) options[(id)kCGImageSourceGenerateImageSpecificLumaScaling]=@NO;
    CGImageRef image = CGImageSourceCreateImageAtIndex(src, 0, (__bridge CFDictionaryRef)options);
    if (!image) return @{ @"image_created":@NO };
    size_t w=CGImageGetWidth(image), h=CGImageGetHeight(image);
    CGDataProviderRef provider=CGImageGetDataProvider(image);
    CFDataRef bytes=provider ? CGDataProviderCopyData(provider) : NULL;
    long providerBytes=bytes ? CFDataGetLength(bytes) : 0;
    NSMutableString *providerHash=[NSMutableString string];
    if(bytes){ unsigned char digest[CC_SHA256_DIGEST_LENGTH]; CC_SHA256(CFDataGetBytePtr(bytes),(CC_LONG)CFDataGetLength(bytes),digest); for(int i=0;i<CC_SHA256_DIGEST_LENGTH;i++) [providerHash appendFormat:@"%02x",digest[i]]; }
    CGColorSpaceRef inputSpace=CGImageGetColorSpace(image);
    NSString *spaceName=inputSpace ? CFBridgingRelease(CGColorSpaceCopyName(inputSpace)) : nil;
    CFDataRef icc=inputSpace ? CGColorSpaceCopyICCData(inputSpace) : NULL;
    NSMutableString *iccHash=[NSMutableString string];
    if(icc){ unsigned char digest[CC_SHA256_DIGEST_LENGTH];
        CC_SHA256(CFDataGetBytePtr(icc),(CC_LONG)CFDataGetLength(icc),digest);
        for(int i=0;i<CC_SHA256_DIGEST_LENGTH;i++) [iccHash appendFormat:@"%02x",digest[i]];
        const char *dir=getenv("PROBE_OUTPUT_DIR");
        if(dir){ NSString *path=[NSString stringWithFormat:@"%s/%@.icc",dir,request==kCGImageSourceDecodeToHDR?@"hdr-returned-colorspace-icc":@"sdr-returned-colorspace-icc"]; [(NSData*)CFBridgingRelease(CFRetain(icc)) writeToFile:path atomically:YES]; }
    }
    if(bytes && getenv("PROBE_RAW")) {
        NSString *file=[NSString stringWithFormat:@"%s-%@.bin",getenv("PROBE_RAW"), request==kCGImageSourceDecodeToHDR ? @"hdr" : @"sdr"];
        [(__bridge NSData*)bytes writeToFile:file atomically:YES];
    }
    if (bytes) CFRelease(bytes);
    if (w*h>16000000) {CGImageRelease(image); return @{@"error":@"probe size bound"};}
    *count=w*h*4; *out=calloc(*count,sizeof(float));
    CGColorSpaceRef space=CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearSRGB);
    CGContextRef ctx=CGBitmapContextCreate(*out,w,h,32,w*16,space,
        kCGBitmapFloatComponents|kCGBitmapByteOrder32Little|kCGImageAlphaPremultipliedLast);
    if(!ctx){ CGColorSpaceRelease(space); CGImageRelease(image); return @{@"error":@"context failed"}; }
    float initialTarget=CGContextGetEDRTargetHeadroom(ctx);
    BOOL targetSet=NO;
    const char *targetText=getenv("PROBE_TARGET_HEADROOM");
    float requestedTarget=initialTarget;
    BOOL targetRequested=(targetText && targetText[0]);
    if(targetRequested) {
        char *end=NULL;
        requestedTarget=strtof(targetText,&end);
        if(end==targetText || *end!='\0' || !isfinite(requestedTarget) || requestedTarget<=0) {
            CGContextRelease(ctx); CGColorSpaceRelease(space); CGImageRelease(image);
            return @{ @"error": @"invalid target headroom" };
        }
        targetSet=CGContextSetEDRTargetHeadroom(ctx,requestedTarget);
    }
    float finalTarget=CGContextGetEDRTargetHeadroom(ctx);
    CGContextDrawImage(ctx,CGRectMake(0,0,w,h),image);
    double lo=INFINITY,hi=-INFINITY,sum=0,alphaLo=INFINITY,alphaHi=-INFINITY;
    size_t nonfinite=0,above=0,nonzero=0;
    for(size_t i=0;i<w*h;i++){
        float a=(*out)[4*i+3];alphaLo=fmin(alphaLo,a);alphaHi=fmax(alphaHi,a);
        for(size_t c=0;c<3;c++) {float v=(*out)[4*i+c];
            if(!isfinite(v)){nonfinite++;continue;}lo=fmin(lo,v);hi=fmax(hi,v);sum+=v;
            above+=v>1.0001f;nonzero+=v!=0;
        }
    }
    NSMutableArray *patches=[NSMutableArray array];
    NSArray *fractions=@[@0.10,@0.30,@0.50,@0.70,@0.90];
    size_t sampleY=h/2;
    for(NSNumber *fraction in fractions){ size_t x=(size_t)floor(w*fraction.doubleValue); if(x>=w)x=w-1; size_t j=(sampleY*w+x)*4;
        [patches addObject:@{@"x":@(x),@"y":@(sampleY),@"fraction":fraction,@"rgba":@[@((*out)[j]),@((*out)[j+1]),@((*out)[j+2]),@((*out)[j+3])]}];
    }
    NSDictionary *result=@{@"patch_centers":patches,@"image_created":@YES,@"width":@(w),@"height":@(h),
        @"context_target_before":@(initialTarget),@"context_target_requested":@(requestedTarget),@"context_target_requested_set":@(targetRequested),@"context_target_after":@(finalTarget),@"context_target_set":@(targetSet),
        @"color_space":spaceName ?: @"unknown",@"input_color_space_present":@(inputSpace!=NULL),@"input_color_space_model":@(inputSpace ? CGColorSpaceGetModel(inputSpace) : -1),@"input_color_space_components":@(inputSpace ? CGColorSpaceGetNumberOfComponents(inputSpace) : 0),@"input_icc_bytes":@(icc ? CFDataGetLength(icc) : 0),@"input_icc_sha256":iccHash,@"bits_per_pixel":@(CGImageGetBitsPerPixel(image)),@"bytes_per_row":@(CGImageGetBytesPerRow(image)),@"bitmap_info":@(CGImageGetBitmapInfo(image)),
        @"bits_per_component":@(CGImageGetBitsPerComponent(image)),@"provider_bytes":@(providerBytes),@"provider_sha256":providerHash,
        @"headroom":@(CGImageGetContentHeadroom(image)),@"rgb_min":@(lo),@"rgb_max":@(hi),
        @"rgb_mean":@(sum/(w*h*3)),@"samples_above_one":@(above),@"nonzero_rgb_samples":@(nonzero),
        @"nonfinite_rgb":@(nonfinite),@"alpha_min":@(alphaLo),@"alpha_max":@(alphaHi),
        @"pixels_decoded":@(providerBytes>0 && alphaLo>0.99 && nonfinite==0)};
    if(icc) CFRelease(icc);CGContextRelease(ctx);CGColorSpaceRelease(space);CGImageRelease(image);return result;
}
int main(int argc,char **argv){@autoreleasepool{
    NSMutableArray *records=[NSMutableArray array];
    for(int i=1;i<argc;i++){
        NSString *path=[NSString stringWithUTF8String:argv[i]];
        CGImageSourceRef src=CGImageSourceCreateWithURL((__bridge CFURLRef)[NSURL fileURLWithPath:path],NULL);
        if(!src){[records addObject:@{@"path":path,@"source_created":@NO}];continue;}
        CFDictionaryRef sourceProperties=NULL;
        NSString *propertyFile=@"";
        const char *outDir=getenv("PROBE_OUTPUT_DIR");
        CFDictionaryRef iso=CGImageSourceCopyAuxiliaryDataInfoAtIndex(src,0,kCGImageAuxiliaryDataTypeISOGainMap);
        CFDictionaryRef apple=CGImageSourceCopyAuxiliaryDataInfoAtIndex(src,0,kCGImageAuxiliaryDataTypeHDRGainMap);
        float *sdr=NULL,*hdr=NULL;size_t ns=0,nh=0;
        NSDictionary *s=decode(src,kCGImageSourceDecodeToSDR,&sdr,&ns);
        NSDictionary *h=decode(src,kCGImageSourceDecodeToHDR,&hdr,&nh);
        // Inspect the ordinary source property/tag directory only after both native decode/draws.
        sourceProperties=CGImageSourceCopyPropertiesAtIndex(src,0,NULL);
        if(sourceProperties&&outDir){
            NSError *propertyError=nil;
            NSData *propertyData=[NSPropertyListSerialization dataWithPropertyList:(__bridge id)sourceProperties format:NSPropertyListXMLFormat_v1_0 options:0 error:&propertyError];
            if(propertyData){ propertyFile=[NSString stringWithFormat:@"%s/source-properties.plist",outDir]; [propertyData writeToFile:propertyFile atomically:YES]; }
        }
        double delta=0; if(ns==nh&&sdr&&hdr){for(size_t j=0;j<ns;j++)if(j%4!=3)delta=fmax(delta,fabs((double)sdr[j]-hdr[j]));}
        [records addObject:@{@"path":path,@"image_count":@(CGImageSourceGetCount(src)),
            @"source_properties_plist":propertyFile,
            @"iso_aux_present":@(iso!=NULL),@"apple_aux_present":@(apple!=NULL),@"sdr":s,@"hdr":h,@"max_sdr_hdr_delta":@(delta)}];
        if(getenv("PROBE_DUMP")) {
            NSString *dir=[NSString stringWithUTF8String:getenv("PROBE_DUMP")];
            if(sdr) [[NSData dataWithBytes:sdr length:ns*sizeof(float)] writeToFile:[dir stringByAppendingPathComponent:[path.lastPathComponent stringByAppendingString:@".sdr.f32"]] atomically:YES];
            if(hdr) [[NSData dataWithBytes:hdr length:nh*sizeof(float)] writeToFile:[dir stringByAppendingPathComponent:[path.lastPathComponent stringByAppendingString:@".hdr.f32"]] atomically:YES];
        }
        free(sdr);free(hdr);if(iso)CFRelease(iso);if(apple)CFRelease(apple);if(sourceProperties)CFRelease(sourceProperties);CFRelease(src);
    }
    NSData *json=[NSJSONSerialization dataWithJSONObject:records options:NSJSONWritingPrettyPrinted error:nil];
    fwrite(json.bytes,1,json.length,stdout);puts("");
}}
