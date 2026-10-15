import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// CS-1: roots remembered by volume, listed with their attachment, and the mount watcher that
/// opens a detached root when its drive arrives.
final class LibraryRootsTests: XCTestCase {
    private func scratch() throws -> URL {
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("roots-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        return temp
    }

    private func jpeg(_ url: URL, shade: UInt8) throws {
        let w = 32, h = 24
        let pixels = [UInt8](repeating: shade, count: w * h * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                          space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }

    func testOpenedFolderIsListedAsAnAttachedBootVolumeRoot() throws {
        let temp = try scratch()
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try jpeg(folder.appendingPathComponent("a.jpg"), shade: 40)
        try jpeg(folder.appendingPathComponent("b.jpg"), shade: 200)
        let support = temp.appendingPathComponent("support")
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let roots = try library.engine.roots().map(LibraryRoot.init)
        let root = try XCTUnwrap(roots.first { $0.path == library.folder?.path })
        XCTAssertTrue(root.attached)
        XCTAssertNil(root.volumeUUID, "a scratch folder is on the boot volume: plain path")
        XCTAssertEqual(root.images, 2)
        XCTAssertEqual(root.name, "shoot")
        XCTAssertEqual(root.listTitle, "shoot")
        XCTAssertEqual(root.id, root.path)
        // A fresh engine on the same app dir lists the same root.
        XCTAssertEqual(try LibraryRoot.list(appSupport: support).map(\.path), roots.map(\.path))
    }

    func testDetachedRootTitleAndIdentityFollowTheVolume() {
        let root = LibraryRoot(path: "/Volumes/Rut photo/2026/wedding", name: "wedding",
                               volumeUUID: "ABCD-1", relativePath: "2026/wedding", volumeName: "Rut photo",
                               attached: false, images: 226)
        XCTAssertEqual(root.listTitle, "wedding · not attached")
        XCTAssertEqual(root.id, "ABCD-1/2026/wedding")
        XCTAssertTrue(root.attachMessage.contains("Rut photo"))
        XCTAssertEqual(root.url.path, "/Volumes/Rut photo/2026/wedding")
    }

    @MainActor
    func testMountWatcherReportsMountsAndUnmounts() {
        let center = NotificationCenter()
        var events: [URL?] = []
        let watcher = VolumeMountWatcher(center: center) { events.append($0) }
        let mounted = URL(fileURLWithPath: "/Volumes/Rut photo-1", isDirectory: true)
        center.post(name: NSWorkspace.didMountNotification, object: nil,
                    userInfo: [NSWorkspace.volumeURLUserInfoKey: mounted])
        center.post(name: NSWorkspace.didUnmountNotification, object: nil, userInfo: nil)
        XCTAssertEqual(events, [mounted, nil])
        withExtendedLifetime(watcher) {}
    }
}
