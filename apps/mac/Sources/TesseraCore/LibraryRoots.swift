import AppKit
import Foundation
import TesseraFFI

/// An indexed root as the engine remembers it (CS-1): a root on an external drive is identified
/// by the volume's UUID and the path relative to the volume, so it opens wherever macOS mounts
/// the drive (`/Volumes/Rut photo-1` included). Boot-volume roots are plain paths.
public struct LibraryRoot: Hashable, Sendable, Identifiable {
    /// Where the root is now, or where it was last seen while not attached.
    public let path: String
    /// The folder's own name.
    public let name: String
    public let volumeUUID: String?
    public let relativePath: String
    /// The drive's name as mounted (nil on the boot volume).
    public let volumeName: String?
    public let attached: Bool
    public let images: Int

    public init(path: String, name: String, volumeUUID: String?, relativePath: String,
                volumeName: String?, attached: Bool, images: Int) {
        self.path = path; self.name = name; self.volumeUUID = volumeUUID; self.relativePath = relativePath
        self.volumeName = volumeName; self.attached = attached; self.images = images
    }

    public init(_ info: RootInfo) {
        self.init(path: info.path, name: info.name, volumeUUID: info.volumeUuid, relativePath: info.relativePath,
                  volumeName: info.volumeName, attached: info.attached, images: Int(info.images))
    }

    /// Volume identity when the root is on a drive; the path otherwise.
    public var id: String { volumeUUID.map { "\($0)/\(relativePath)" } ?? path }
    public var url: URL { URL(fileURLWithPath: path, isDirectory: true) }
    /// "Rut photo · not attached" for the folder popover.
    public var listTitle: String { attached ? name : "\(name) · not attached" }
    /// What the person has to do to open a detached root.
    public var attachMessage: String {
        "Attach “\(volumeName ?? name)” to open \(name) (last seen at \(path))"
    }

    /// Every root the per-machine index knows, with their current attachment. Blocking
    /// (opens the index): call off the main actor.
    public static func list(appSupport: URL? = nil) throws -> [LibraryRoot] {
        let engine = try Engine.open(appSupportDir: (appSupport ?? EngineLibrary.defaultSupportDirectory).path)
        return try engine.roots().map(LibraryRoot.init)
    }
}

/// Reports drives mounting and unmounting (`NSWorkspace.didMountNotification`), so a root
/// whose volume was absent opens the moment it is attached.
@MainActor
public final class VolumeMountWatcher {
    /// Observer tokens, removed on `stop()` (or when the watcher is dropped).
    private let observers: Observers
    private let center: NotificationCenter

    private final class Observers: @unchecked Sendable {
        var tokens: [any NSObjectProtocol] = []
        func remove(from center: NotificationCenter) {
            for token in tokens { center.removeObserver(token) }
            tokens.removeAll()
        }
    }

    /// `onChange` runs on the main actor after a mount or unmount; `mounted` is the new
    /// volume's URL for mounts, nil for unmounts.
    public init(center: NotificationCenter = NSWorkspace.shared.notificationCenter,
                onChange: @escaping @MainActor (_ mounted: URL?) -> Void) {
        self.center = center
        observers = Observers()
        for name in [NSWorkspace.didMountNotification, NSWorkspace.didUnmountNotification] {
            observers.tokens.append(center.addObserver(forName: name, object: nil, queue: .main) { note in
                let mounted = note.name == NSWorkspace.didMountNotification
                    ? note.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL : nil
                MainActor.assumeIsolated { onChange(mounted) }
            })
        }
    }

    public func stop() { observers.remove(from: center) }

    deinit {
        let (observers, center) = (self.observers, self.center)
        observers.remove(from: center)
    }
}
