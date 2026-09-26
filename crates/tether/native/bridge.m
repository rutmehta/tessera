// All entry points and delegate callbacks run on the process main thread.
// No Objective-C object ever stores a Rust pointer. C callbacks below are
// synchronous visitors, never ImageCaptureCore completion callbacks.
#import <Foundation/Foundation.h>
#import <ImageCaptureCore/ImageCaptureCore.h>
#include <stdlib.h>
#include <string.h>

static BOOL tether_should_download(BOOL active, BOOL ready, BOOL newCapture, BOOL seen) {
    return active && ready && newCapture && !seen;
}

static void pump(NSTimeInterval seconds, BOOL (^done)(void)) {
    NSTimeInterval deadline = NSProcessInfo.processInfo.systemUptime + seconds;
    while (!done() && NSProcessInfo.processInfo.systemUptime < deadline) {
        @autoreleasepool {
            // A short slice also bounds callers whose run loop has no sources.
            NSDate *until = [NSDate dateWithTimeIntervalSinceNow:0.01];
            if (![NSRunLoop.mainRunLoop runMode:NSDefaultRunLoopMode beforeDate:until])
                [NSThread sleepForTimeInterval:0.001];
        }
    }
}

static char *message(NSString *text) { return strdup(text.UTF8String); }
static char *thread_error(void) {
    return NSThread.isMainThread ? NULL : message(@"ImageCaptureCore must run on the process main thread");
}

@interface TetherSession : NSObject <ICCameraDeviceDelegate, ICCameraDeviceDownloadDelegate>
@property(nonatomic, strong) ICCameraDevice *camera;
@property(nonatomic, strong) NSURL *folder;
@property(nonatomic, strong) NSMutableArray<NSString *> *paths;
@property(nonatomic, strong) NSMutableArray<NSString *> *errors;
@property(nonatomic, strong) NSMutableSet<ICCameraItem *> *seen;
// The SDK uses assign delegates. Retain ourselves across ALL outstanding
// requests, even after the Rust owner drops. If a broken driver never replies,
// this inert island is deliberately retained rather than risking use-after-free.
@property(nonatomic, strong) TetherSession *keepAlive;
@property(nonatomic) BOOL opening;
@property(nonatomic) BOOL opened;
@property(nonatomic) BOOL closing;
@property(nonatomic) BOOL ready;
@property(nonatomic) BOOL active;
@property(nonatomic) BOOL stopping;
@property(nonatomic) NSUInteger downloads;
- (void)begin;
- (void)shutdown;
- (BOOL)settled;
- (void)releaseIfSettled;
@end

@implementation TetherSession
- (instancetype)init {
    if ((self = [super init])) {
        _paths = [NSMutableArray array];
        _errors = [NSMutableArray array];
        _seen = [NSMutableSet set];
    }
    return self;
}
- (void)record:(NSError *)error {
    if (error) [self.errors addObject:error.localizedDescription ?: @"ImageCaptureCore error"];
}
- (BOOL)settled { return !self.opening && !self.opened && !self.closing && self.downloads == 0; }
- (void)releaseIfSettled {
    if (self.stopping && self.settled) {
        self.camera.delegate = nil;
        self.keepAlive = nil;
    }
}
- (void)begin {
    self.keepAlive = self;
    self.camera.delegate = self;
    self.opening = YES;
    [self.camera requestOpenSession];
}
- (void)shutdown {
    self.active = NO;
    self.stopping = YES;
    if (self.downloads) [self.camera cancelDownload];
    if (self.opened && !self.closing) {
        self.closing = YES;
        [self.camera requestCloseSession];
    }
    [self releaseIfSettled];
}
- (void)device:(ICDevice *)device didOpenSessionWithError:(NSError *)error {
    self.opening = NO;
    self.opened = error == nil;
    [self record:error];
    if (self.stopping || error) [self shutdown];
}
- (void)device:(ICDevice *)device didCloseSessionWithError:(NSError *)error {
    BOOL unexpected = !self.stopping;
    self.closing = NO;
    self.opened = NO;
    self.active = NO;
    self.ready = NO;
    self.stopping = YES;
    [self record:error];
    if (unexpected && !error) [self.errors addObject:@"Camera session closed unexpectedly"];
    [self releaseIfSettled];
}
- (void)didRemoveDevice:(ICDevice *)device {
    [self.errors addObject:@"Camera disconnected"];
    self.active = NO;
    self.ready = NO;
    self.stopping = YES;
    // Outstanding open/download requests retain their callback target until
    // their own completions arrive; removal is not proof they were cancelled.
    self.opened = NO;
    [self releaseIfSettled];
}
- (void)device:(ICDevice *)device didEncounterError:(NSError *)error { [self record:error]; }
- (void)deviceDidBecomeReadyWithCompleteContentCatalog:(ICCameraDevice *)camera {
    if (camera == self.camera && !self.stopping) self.ready = YES;
}
- (void)cameraDevice:(ICCameraDevice *)camera didAddItems:(NSArray<ICCameraItem *> *)items {
    if (camera != self.camera) return;
    for (ICCameraItem *item in items) {
        BOOL seen = [self.seen containsObject:item];
        [self.seen addObject:item];
        // The SDK's flag excludes inventory AND newly mounted stores. Merely
        // seeing didAddItems after start is not sufficient to identify a shot.
        if (![item isKindOfClass:ICCameraFile.class] ||
            !tether_should_download(self.active, self.ready,
                                    item.wasAddedAfterContentCatalogCompleted, seen)) continue;
        self.downloads++;
        [camera requestDownloadFile:(ICCameraFile *)item
                            options:@{ICDownloadsDirectoryURL: self.folder,
                                      ICOverwrite: @NO,
                                      ICDeleteAfterSuccessfulDownload: @NO,
                                      ICDownloadSidecarFiles: @NO}
                   downloadDelegate:self
                didDownloadSelector:@selector(didDownloadFile:error:options:contextInfo:)
                        contextInfo:NULL];
    }
}
- (void)didDownloadFile:(ICCameraFile *)file error:(NSError *)error
               options:(NSDictionary<NSString *, id> *)options contextInfo:(void *)contextInfo {
    if (self.downloads) self.downloads--;
    [self record:error];
    if (!error) {
        NSString *name = options[ICSavedFilename];
        if (![name isKindOfClass:NSString.class] || name.length == 0 ||
            ![name isEqualToString:name.lastPathComponent] ||
            [name isEqualToString:@"."] || [name isEqualToString:@".."]) {
            [self.errors addObject:@"Download completed without a valid saved filename"];
        } else {
            NSURL *url = [self.folder URLByAppendingPathComponent:name];
            BOOL directory = NO;
            if ([NSFileManager.defaultManager fileExistsAtPath:url.path isDirectory:&directory] && !directory)
                [self.paths addObject:url.path];
            else
                [self.errors addObject:@"Download completion did not produce a file"];
        }
    }
    [self releaseIfSettled];
}
- (void)cameraDevice:(ICCameraDevice *)camera didRemoveItems:(NSArray<ICCameraItem *> *)items {
    [self.seen minusSet:[NSSet setWithArray:items]];
}
- (void)cameraDevice:(ICCameraDevice *)camera didRenameItems:(NSArray<ICCameraItem *> *)items {}
- (void)cameraDeviceDidChangeCapability:(ICCameraDevice *)camera {}
- (void)cameraDevice:(ICCameraDevice *)camera didReceivePTPEvent:(NSData *)eventData {}
- (void)cameraDevice:(ICCameraDevice *)camera didReceiveThumbnail:(CGImageRef)thumbnail
             forItem:(ICCameraItem *)item error:(NSError *)error { [self record:error]; }
- (void)cameraDevice:(ICCameraDevice *)camera didReceiveMetadata:(NSDictionary *)metadata
             forItem:(ICCameraItem *)item error:(NSError *)error { [self record:error]; }
- (void)cameraDeviceDidRemoveAccessRestriction:(ICDevice *)device {}
- (void)cameraDeviceDidEnableAccessRestriction:(ICDevice *)device {
    [self.errors addObject:@"Camera access is restricted; unlock and trust this computer"];
}
@end

@interface TetherContext : NSObject <ICDeviceBrowserDelegate>
@property(nonatomic, strong) ICDeviceBrowser *browser;
@property(nonatomic, strong) NSMutableArray<ICCameraDevice *> *cameras;
@property(nonatomic, strong) TetherSession *session;
@property(nonatomic) BOOL enumerated;
@end
@implementation TetherContext
- (instancetype)init {
    if ((self = [super init])) {
        _cameras = [NSMutableArray array];
        _browser = [ICDeviceBrowser new];
        _browser.browsedDeviceTypeMask = (ICDeviceTypeMask)(ICDeviceTypeMaskCamera | ICDeviceLocationTypeMaskLocal);
        _browser.delegate = self;
        [_browser start];
    }
    return self;
}
- (void)deviceBrowser:(ICDeviceBrowser *)browser didAddDevice:(ICDevice *)device moreComing:(BOOL)more {
    if ([device isKindOfClass:ICCameraDevice.class] && ![self.cameras containsObject:(ICCameraDevice *)device])
        [self.cameras addObject:(ICCameraDevice *)device];
}
- (void)deviceBrowser:(ICDeviceBrowser *)browser didRemoveDevice:(ICDevice *)device moreGoing:(BOOL)more {
    if ([device isKindOfClass:ICCameraDevice.class]) [self.cameras removeObject:(ICCameraDevice *)device];
}
- (void)deviceBrowserDidEnumerateLocalDevices:(ICDeviceBrowser *)browser { self.enumerated = YES; }
@end

static char *take_error(TetherContext *context) {
    NSString *error = context.session.errors.firstObject;
    if (!error) return NULL;
    [context.session.errors removeObjectAtIndex:0];
    return message(error);
}
static void discover(TetherContext *context) {
    // Refresh even after initial enumeration; never wait indefinitely for a
    // callback when there are no attached cameras.
    pump(context.enumerated ? 0.05 : 2.0, ^BOOL { return NO; });
}

void *tessera_tether_new(void) {
    @autoreleasepool {
        if (!NSThread.isMainThread) return NULL;
        return (__bridge_retained void *)[TetherContext new];
    }
}
void tessera_tether_string_free(char *value) { free(value); }

typedef void (*DeviceVisitor)(void *, const char *, const char *, int);
typedef void (*PathVisitor)(void *, const char *);
char *tessera_tether_devices(void *handle, DeviceVisitor visit, void *user) {
    @autoreleasepool {
        char *error = thread_error(); if (error) return error;
        TetherContext *context = (__bridge TetherContext *)handle;
        discover(context);
        if ((error = take_error(context))) return error;
        for (ICCameraDevice *camera in context.cameras) {
            NSString *identifier = camera.UUIDString ?: camera.persistentIDString ?:
                [NSString stringWithFormat:@"usb:%d:%d:%d", camera.usbVendorID, camera.usbProductID, camera.usbLocationID];
            visit(user, identifier.UTF8String, (camera.name ?: @"Camera").UTF8String,
                  [camera.capabilities containsObject:ICCameraDeviceCanTakePicture]);
        }
        return NULL;
    }
}
char *tessera_tether_start(void *handle, const char *folder) {
    @autoreleasepool {
        char *error = thread_error(); if (error) return error;
        TetherContext *context = (__bridge TetherContext *)handle;
        if (context.session && !context.session.settled)
            return message(@"A camera session is already open or is still closing");
        if ((error = take_error(context))) return error;
        discover(context);
        if (context.cameras.count == 0) return message(@"No camera found");
        if (context.cameras.count > 1) return message(@"Multiple cameras found; connect only one camera to start tethering");
        NSString *path = [NSFileManager.defaultManager stringWithFileSystemRepresentation:folder length:strlen(folder)];
        BOOL directory = NO;
        if (!path || ![NSFileManager.defaultManager fileExistsAtPath:path isDirectory:&directory] || !directory)
            return message(@"Download folder does not exist or is not a directory");
        TetherSession *session = [TetherSession new];
        session.camera = context.cameras.firstObject;
        session.folder = [NSURL fileURLWithPath:path isDirectory:YES];
        context.session = session;
        [session begin];
        pump(15.0, ^BOOL { return (session.opened && session.ready) || session.errors.count != 0; });
        if ((error = take_error(context))) { [session shutdown]; return error; }
        if (!session.opened || !session.ready) {
            [session shutdown];
            return message(@"Timed out opening camera session or enumerating its content catalog");
        }
        session.active = YES;
        return NULL;
    }
}
char *tessera_tether_capture(void *handle) {
    @autoreleasepool {
        char *error = thread_error(); if (error) return error;
        TetherContext *context = (__bridge TetherContext *)handle;
        if ((error = take_error(context))) return error;
        TetherSession *session = context.session;
        if (!session.active || !session.ready || !session.opened)
            return message(@"Start a camera session before requesting capture");
        if (![session.camera.capabilities containsObject:ICCameraDeviceCanTakePicture])
            return message(@"This camera does not support remote capture");
        [session.camera requestTakePicture];
        // Command acceptance is asynchronous. Later driver/download failures
        // are returned by poll(), not disguised as successful downloaded shots.
        return take_error(context);
    }
}
char *tessera_tether_poll(void *handle, PathVisitor visit, void *user) {
    @autoreleasepool {
        char *error = thread_error(); if (error) return error;
        TetherContext *context = (__bridge TetherContext *)handle;
        pump(0.01, ^BOOL { return NO; });
        if ((error = take_error(context))) return error;
        for (NSString *path in context.session.paths) visit(user, path.fileSystemRepresentation);
        [context.session.paths removeAllObjects];
        return NULL;
    }
}
char *tessera_tether_stop(void *handle) {
    @autoreleasepool {
        char *error = thread_error(); if (error) return error;
        TetherContext *context = (__bridge TetherContext *)handle;
        TetherSession *session = context.session;
        if (!session) return NULL;
        // Finish already accepted transfers (including RAW+JPEG companions)
        // before closing. On timeout shutdown cancels, with an explicit error.
        session.active = NO;
        pump(30.0, ^BOOL { return session.downloads == 0 || session.errors.count != 0; });
        BOOL unfinished = session.downloads != 0;
        [session shutdown];
        pump(5.0, ^BOOL { return session.settled; });
        if ((error = take_error(context))) return error;
        if (unfinished) return message(@"Timed out waiting for camera downloads during shutdown");
        if (!session.settled) return message(@"Timed out closing camera session; outstanding callbacks remain safely retained");
        return NULL;
    }
}
void tessera_tether_free(void *handle) {
    @autoreleasepool {
        // Rust's !Send/!Sync marker and constructor check enforce this.
        NSCAssert(NSThread.isMainThread, @"Tether backend dropped off main thread");
        TetherContext *context = (__bridge_transfer TetherContext *)handle;
        context.browser.delegate = nil;
        [context.browser stop];
        [context.session shutdown];
        // No pump is needed: pending requests own their delegate, and its
        // callbacks only access Objective-C state, never the freed Rust owner.
    }
}

#ifdef TETHER_BRIDGE_TEST
// Lifetime tests invoke delegate state transitions without attached hardware;
// the unused device/file arguments are intentionally nil.
#pragma clang diagnostic ignored "-Wnonnull"
#include <assert.h>
#include <stdio.h>

static void test_device(void *user, const char *identifier, const char *name, int capture) {
    assert(identifier && name);
    (*(unsigned *)user)++;
}
static void test_path(void *user, const char *path) { (*(unsigned *)user)++; }

int main(void) {
    @autoreleasepool {
        assert(!tether_should_download(YES, YES, NO, NO));
        assert(!tether_should_download(YES, NO, YES, NO));
        assert(!tether_should_download(NO, YES, YES, NO));
        assert(!tether_should_download(YES, YES, YES, YES));
        assert(tether_should_download(YES, YES, YES, NO));

        // Drop while opening: the callback target survives until a late open
        // result and the subsequent close have both arrived.
        __weak TetherSession *weakSession;
        @autoreleasepool {
            TetherSession *session = [TetherSession new];
            weakSession = session;
            [session begin];
            [session shutdown];
        }
        assert(weakSession != nil);
        [weakSession device:nil didOpenSessionWithError:nil];
        assert(weakSession != nil);
        [weakSession device:nil didCloseSessionWithError:nil];
        assert(weakSession == nil);

        // Closing the session isn't enough when a download still owns a
        // selector callback. Its completion must also drain before release.
        @autoreleasepool {
            TetherSession *session = [TetherSession new];
            weakSession = session;
            [session begin];
            [session device:nil didOpenSessionWithError:nil];
            session.downloads = 1;
            [session shutdown];
            [session device:nil didCloseSessionWithError:nil];
        }
        assert(weakSession != nil);
        [weakSession didDownloadFile:nil error:[NSError errorWithDomain:@"test" code:1 userInfo:nil]
                             options:@{} contextInfo:NULL];
        assert(weakSession == nil);

        TetherSession *session = [TetherSession new];
        [session didDownloadFile:nil error:nil options:@{ICSavedFilename: @"../escape.raw"} contextInfo:NULL];
        assert(session.errors.count == 1);
        assert(session.paths.count == 0);

        // Opt-in real SDK smoke test. Never opens or captures from hardware.
        if (getenv("TETHER_LIVE_SMOKE")) {
            void *handle = tessera_tether_new();
            assert(handle);
            unsigned count = 0;
            char *error = tessera_tether_devices(handle, test_device, &count);
            assert(!error);
            printf("ImageCaptureCore discovery: %u camera(s)\n", count);
            unsigned paths = 0;
            error = tessera_tether_poll(handle, test_path, &paths);
            assert(!error && paths == 0);
            error = tessera_tether_capture(handle);
            assert(error); // capture without a session is rejected
            tessera_tether_string_free(error);
            error = tessera_tether_stop(handle);
            assert(!error);
            tessera_tether_free(handle);
        }
        puts("Bridge policy/lifetime tests passed");
    }
    return 0;
}
#endif
