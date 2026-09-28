#import <AppKit/AppKit.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <QuartzCore/QuartzCore.h>
#include <string.h>
#import <CoreVideo/CoreVideo.h>
#import <mach/mach_time.h>
#import <CommonCrypto/CommonDigest.h>
#include <math.h>
#include <stdio.h>
#include <unistd.h>

// SOURCE-ONLY capability helper. No permission request, no whole-desktop filter,
// no input/revision matching and no P11 acceptance evaluator.
static NSString *const Scope = @"FILTERED selected-window API/pixels/timestamps capability only; not proof of unobscured visible detail or P11 latency.";
static BOOL WriteJSON(id value, NSString *path) {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:NSJSONWritingPrettyPrinted | NSJSONWritingSortedKeys error:&error];
    return data && [data writeToFile:path options:NSDataWritingAtomic error:&error];
}
static int Fail(NSString *out, int code, NSString *reason) {
    WriteJSON(@{@"scope":Scope,@"direct_exit":@(code),@"error":reason,@"capture_started":@NO},[out stringByAppendingPathComponent:@"error.json"]);
    return code;
}
static NSString *SHA(NSData *data) {
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(data.bytes, (CC_LONG)data.length, digest);
    NSMutableString *s = [NSMutableString string];
    for (NSUInteger i=0; i<sizeof(digest); ++i) [s appendFormat:@"%02x",digest[i]];
    return s;
}
static NSDictionary *RectJSON(CGRect r) {
    return @{@"x":@(r.origin.x),@"y":@(r.origin.y),@"width":@(r.size.width),@"height":@(r.size.height)};
}
static id Geometry(id value) {
    if (!value) return [NSNull null];
    if ([value isKindOfClass:[NSNumber class]] || [value isKindOfClass:[NSString class]]) return value;
    if ([value isKindOfClass:[NSDictionary class]]) return value;
    if ([value isKindOfClass:[NSValue class]] && strcmp([value objCType], @encode(CGRect))==0) {
        CGRect rect; [value getValue:&rect size:sizeof(rect)]; return RectJSON(rect);
    }
    return @{ @"unparsed_description": [value description] };
}
static NSString *Canonical(NSString *path) { return path.stringByStandardizingPath.stringByResolvingSymlinksInPath; }
static double Now(void) { return CACurrentMediaTime(); }
static BOOL Pump(BOOL (^done)(void), double deadline) {
    while (!done() && Now()<deadline) [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    return done();
}

@interface Capability : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic,strong) NSDictionary *config;
@property(nonatomic,copy) NSString *out;
@property CGRect roiGlobal;
@property CGRect windowFrame;
@property BOOL accepting;
@property BOOL failed;
@property(nonatomic,copy) NSString *failure;
@property(nonatomic,strong) NSMutableArray *events;
@property NSUInteger saved;
@property NSUInteger callbacks;
@property(nonatomic,strong) SCStream *stream;
- (BOOL)guardIdentity;
@end
@implementation Capability
- (BOOL)guardIdentity {
    pid_t pid=[self.config[@"pid"] intValue];
    NSRunningApplication *app=[NSRunningApplication runningApplicationWithProcessIdentifier:pid];
    NSString *expectedPath=Canonical(self.config[@"app_path"]);
    BOOL valid=app && !app.terminated && app.activationPolicy==NSApplicationActivationPolicyRegular &&
        [app.bundleIdentifier isEqual:self.config[@"bundle_id"]] &&
        [Canonical(app.bundleURL.path ?: @"") isEqual:expectedPath] && app.launchDate &&
        fabs(app.launchDate.timeIntervalSince1970-[self.config[@"launch_epoch"] doubleValue])<0.000001 &&
        NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier==pid;
    NSArray *rows=CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionIncludingWindow,[self.config[@"window_id"] unsignedIntValue]));
    NSDictionary *row=rows.count==1?rows[0]:nil;
    CGRect frame=CGRectNull;
    valid=valid && row && [row[(__bridge NSString *)kCGWindowOwnerPID] intValue]==pid &&
        [row[(__bridge NSString *)kCGWindowLayer] intValue]==0 &&
        [row[(__bridge NSString *)kCGWindowIsOnscreen] boolValue] &&
        CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)row[(__bridge NSString *)kCGWindowBounds],&frame) &&
        CGRectContainsRect(frame,self.roiGlobal) && CGRectEqualToRect(frame,self.windowFrame);
    if (!valid) { self.failed=YES; self.failure=@"test app identity/foreground/window geometry guard failed"; self.accepting=NO; }
    return valid;
}
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    dispatch_async(dispatch_get_main_queue(), ^{ self.failed=YES; self.failure=error.description; self.accepting=NO; });
}
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample ofType:(SCStreamOutputType)type {
    if (type!=SCStreamOutputTypeScreen || !CMSampleBufferIsValid(sample)) return;
    // Retain one sample until the main queue handles it. The configured queue is
    // bounded; serial synchronous delivery prevents an unbounded pending copy queue.
    CFRetain(sample);
    dispatch_sync(dispatch_get_main_queue(), ^{
        @autoreleasepool {
            if (!self.accepting || self.callbacks>=256) { CFRelease(sample); return; }
            ++self.callbacks;
            if (![self guardIdentity]) { CFRelease(sample); return; }
            NSArray *attachments=(__bridge NSArray *)CMSampleBufferGetSampleAttachmentsArray(sample, false);
            NSDictionary *info=attachments.count?attachments[0]:@{};
            NSNumber *status=info[SCStreamFrameInfoStatus];
            NSNumber *displayTime=info[SCStreamFrameInfoDisplayTime];
            NSMutableDictionary *event=[@{@"callback_mach_absolute_ticks":@(mach_absolute_time()),
                @"status":status ?: (id)[NSNull null], @"display_time_mach_absolute_ticks":displayTime ?: (id)[NSNull null],
                @"scale_factor":Geometry(info[SCStreamFrameInfoScaleFactor]),
                @"content_scale":Geometry(info[SCStreamFrameInfoContentScale]),
                @"content_rect":Geometry(info[SCStreamFrameInfoContentRect]),
                @"screen_rect":Geometry(info[SCStreamFrameInfoScreenRect]),
                @"saved_pixels":@NO} mutableCopy];
            CVPixelBufferRef pixel=CMSampleBufferGetImageBuffer(sample);
            if (status && status.integerValue==SCFrameStatusComplete && displayTime.unsignedLongLongValue>0 && pixel && self.saved<8) {
                size_t width=CVPixelBufferGetWidth(pixel),height=CVPixelBufferGetHeight(pixel),stride=CVPixelBufferGetBytesPerRow(pixel);
                if (CVPixelBufferGetPixelFormatType(pixel)!=kCVPixelFormatType_32BGRA || CVPixelBufferIsPlanar(pixel) || width==0 || height==0 || width>1024 || height>1024 || stride<width*4 || CVPixelBufferLockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly)!=kCVReturnSuccess) {
                    self.failed=YES;self.failure=@"unexpected pixel layout or pixel-lock failure";self.accepting=NO;
                } else {
                    NSMutableData *packed=[NSMutableData dataWithLength:width*height*4];
                    const unsigned char *base=CVPixelBufferGetBaseAddress(pixel);
                    if (!base) {
                        CVPixelBufferUnlockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly);
                        self.failed=YES;self.failure=@"pixel buffer base address is NULL after successful lock";self.accepting=NO;
                        event[@"error"]=self.failure;[self.events addObject:event];
                        WriteJSON(self.events,[self.out stringByAppendingPathComponent:@"frames.json"]);
                        CFRelease(sample);return;
                    }
                    for (size_t y=0;y<height;++y) memcpy((unsigned char *)packed.mutableBytes+y*width*4,base+y*stride,width*4);
                    CVPixelBufferUnlockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly);
                    NSString *name=[NSString stringWithFormat:@"frame-%03lu.bgra",(unsigned long)self.saved];
                    NSError *error=nil;
                    if (![packed writeToFile:[self.out stringByAppendingPathComponent:name] options:NSDataWritingAtomic error:&error]) {
                        self.failed=YES;self.failure=error.description;self.accepting=NO;
                    } else {
                        event[@"saved_pixels"]=@YES;event[@"file"]=name;event[@"sha256"]=SHA(packed);
                        event[@"width"]=@(width);event[@"height"]=@(height);event[@"source_bytes_per_row"]=@(stride);
                        event[@"saved_bytes_per_row"]=@(width*4);event[@"pixel_format"]=@"BGRA8";++self.saved;
                    }
                }
            }
            [self.events addObject:event];
            if (!WriteJSON(self.events,[self.out stringByAppendingPathComponent:@"frames.json"])) { self.failed=YES;self.failure=@"frame metadata write failed";self.accepting=NO; }
            CFRelease(sample);
        }
    });
}
@end

int main(int argc,const char *argv[]) {
    @autoreleasepool {
        if (argc!=2) { fprintf(stderr,"usage: filtered-capture-capability config.json\n");return 2; }
        NSData *raw=[NSData dataWithContentsOfFile:@(argv[1])];
        NSDictionary *c=raw?[NSJSONSerialization JSONObjectWithData:raw options:0 error:nil]:nil;
        NSArray *strings=@[@"output_dir",@"bundle_id",@"app_path"];
        if (![c isKindOfClass:[NSDictionary class]]) return 2;
        for (NSString *key in strings) if (![c[key] isKindOfClass:[NSString class]] || [c[key] length]==0) return 2;
        for (NSString *key in @[@"pid",@"launch_epoch",@"window_id",@"display_id"]) if (![c[key] isKindOfClass:[NSNumber class]] || !isfinite([c[key] doubleValue]) || [c[key] doubleValue]<=0) return 2;
        NSDictionary *r=c[@"roi_display_points"];
        if (![r isKindOfClass:[NSDictionary class]]) return 2;
        for (NSString *key in @[@"x",@"y",@"width",@"height"]) if (![r[key] isKindOfClass:[NSNumber class]] || !isfinite([r[key] doubleValue])) return 2;
        CGRect roi=CGRectMake([r[@"x"] doubleValue],[r[@"y"] doubleValue],[r[@"width"] doubleValue],[r[@"height"] doubleValue]);
        if (roi.origin.x<0 || roi.origin.y<0 || roi.size.width<=0 || roi.size.height<=0 || roi.size.width>512 || roi.size.height>512) return 2;
        NSString *out=Canonical(c[@"output_dir"]);
        if (![out hasPrefix:@"/Volumes/betterSSD/"] || [NSFileManager.defaultManager fileExistsAtPath:out] || ![c[@"bundle_id"] hasPrefix:@"dev.tessera.m258."] || ![Canonical(c[@"app_path"]) hasPrefix:@"/Volumes/betterSSD/"] || ![Canonical(c[@"app_path"]) hasSuffix:@".app"]) return 2;
        NSError *mkdirError=nil;
        if (![NSFileManager.defaultManager createDirectoryAtPath:out withIntermediateDirectories:NO attributes:nil error:&mkdirError]) return 2;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,9*NSEC_PER_SEC),dispatch_get_global_queue(QOS_CLASS_UTILITY,0),^{
            WriteJSON(@{@"scope":Scope,@"error":@"9 second whole-process watchdog; capture capability incomplete"},[out stringByAppendingPathComponent:@"watchdog.json"]);
            _exit(124);
        });
        mach_timebase_info_data_t tb={0,0};
        if (mach_timebase_info(&tb)!=KERN_SUCCESS || tb.denom==0) return Fail(out,3,@"mach timebase unavailable");
        BOOL permission=CGPreflightScreenCaptureAccess();
        if (!WriteJSON(@{@"scope":Scope,@"configuration":c,@"permission_for_this_process":@(permission),@"permission_requested":@NO,
                         @"mach_timebase_numer":@(tb.numer),@"mach_timebase_denom":@(tb.denom),@"started_mach_absolute_ticks":@(mach_absolute_time())},[out stringByAppendingPathComponent:@"preflight.json"])) return 3;
        if (!permission) return Fail(out,4,@"screen capture permission unavailable for this process; no request made");
        __block SCShareableContent *content=nil;__block NSError *contentError=nil;__block BOOL resolved=NO;
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES completionHandler:^(SCShareableContent *value,NSError *error){
            dispatch_async(dispatch_get_main_queue(),^{content=value;contentError=error;resolved=YES;});
        }];
        if (!Pump(^BOOL{return resolved;},Now()+2) || contentError || !content) { WriteJSON(@{@"error":contentError.description?:@"content enumeration timeout"},[out stringByAppendingPathComponent:@"error.json"]);return 5; }
        SCDisplay *display=nil;SCWindow *window=nil;
        for (SCDisplay *d in content.displays) if (d.displayID==[c[@"display_id"] unsignedIntValue]) display=d;
        for (SCWindow *w in content.windows) if (w.windowID==[c[@"window_id"] unsignedIntValue]) window=w;
        if (!display || !window || window.owningApplication.processID!=[c[@"pid"] intValue] || ![window.owningApplication.bundleIdentifier isEqual:c[@"bundle_id"]] || !window.onScreen || window.windowLayer!=0 || !CGRectContainsRect(CGRectMake(0,0,display.frame.size.width,display.frame.size.height),roi)) return Fail(out,6,@"selected display/window/owner/onscreen/layer or ROI validation failed");
        CGRect global=CGRectOffset(roi,display.frame.origin.x,display.frame.origin.y);
        if (!CGRectContainsRect(window.frame,global)) return Fail(out,6,@"ROI is outside the selected test window");
        Capability *cap=[Capability new];cap.config=c;cap.out=out;cap.roiGlobal=global;cap.windowFrame=window.frame;cap.events=[NSMutableArray array];
        if (![cap guardIdentity]) return Fail(out,6,cap.failure?:@"test app identity guard failed");
        SCContentFilter *filter=[[SCContentFilter alloc] initWithDisplay:display includingWindows:@[window]];
        filter.includeMenuBar=NO;
        SCStreamConfiguration *config=[SCStreamConfiguration new];config.sourceRect=roi;config.width=(size_t)ceil(roi.size.width);config.height=(size_t)ceil(roi.size.height);
        config.pixelFormat=kCVPixelFormatType_32BGRA;config.showsCursor=NO;config.capturesAudio=NO;config.minimumFrameInterval=CMTimeMake(1,30);config.queueDepth=3;
        cap.stream=[[SCStream alloc] initWithFilter:filter configuration:config delegate:cap];
        NSError *outputError=nil;
        dispatch_queue_t queue=dispatch_queue_create("tessera.filtered-capture-capability.samples",DISPATCH_QUEUE_SERIAL);
        if (![cap.stream addStreamOutput:cap type:SCStreamOutputTypeScreen sampleHandlerQueue:queue error:&outputError]) return Fail(out,7,outputError.description?:@"addStreamOutput failed");
        if (!WriteJSON(@{@"scope":Scope,@"display_frame":RectJSON(display.frame),@"window_frame":RectJSON(window.frame),@"roi_display_points":RectJSON(roi),@"roi_global_points":RectJSON(global),@"output_width":@(config.width),@"output_height":@(config.height),@"maximum_saved_frames":@8,@"maximum_callbacks":@256,@"capture_seconds":@2,@"filter":@"display including only exact test window; menu bar/cursor/audio excluded"},[out stringByAppendingPathComponent:@"stream-config.json"])) return Fail(out,7,@"stream configuration metadata write failed before capture start");
        __block BOOL started=NO;__block NSError *startError=nil;
        cap.accepting=YES;
        [cap.stream startCaptureWithCompletionHandler:^(NSError *error){dispatch_async(dispatch_get_main_queue(),^{startError=error;started=YES;});}];
        BOOL startCompleted=Pump(^BOOL{return started||cap.failed;},Now()+2);
        if (startCompleted && started && !startError && !cap.failed) Pump(^BOOL{return cap.failed;},Now()+2);
        cap.accepting=NO;
        __block BOOL stopped=NO;__block NSError *stopError=nil;
        [cap.stream stopCaptureWithCompletionHandler:^(NSError *error){dispatch_async(dispatch_get_main_queue(),^{stopError=error;stopped=YES;});}];
        BOOL stopCompleted=Pump(^BOOL{return stopped;},Now()+1);
        NSDictionary *result=@{@"scope":Scope,@"start_completed":@(startCompleted&&started),@"start_error":startError.description?:[NSNull null],
            @"stop_completed":@(stopCompleted),@"stop_error":stopError.description?:[NSNull null],@"guard_failed":@(cap.failed),@"failure":cap.failure?:[NSNull null],
            @"callbacks":@(cap.callbacks),@"saved_complete_frames":@(cap.saved),@"finished_mach_absolute_ticks":@(mach_absolute_time()),@"p11_acceptance":@"not evaluated"};
        BOOL written=WriteJSON(result,[out stringByAppendingPathComponent:@"result.json"]);
        return written && startCompleted && started && !startError && stopCompleted && !stopError && !cap.failed && cap.saved>0 ? 0:8;
    }
}
