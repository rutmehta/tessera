import Foundation

/// What kind of file backs an item. `synthetic` items have no file (generated stub data).
public enum PhotoKind: String, Sendable {
    case raw = "RAW"
    case jpeg = "JPEG"
    case png = "PNG"
    case heif = "HEIF"
    case tiff = "TIFF"
    case synthetic = "STUB"
}

/// One image in the library. Immutable value; per-image user state lives in `CullStore`.
public struct PhotoItem: Sendable, Hashable, Identifiable {
    /// Dense index into `StubLibrary.items` (items are sorted by capture time).
    public let id: Int
    public let url: URL?
    public let name: String
    public let kind: PhotoKind
    public let captureDate: Date
    /// Oriented pixel size, 0 when unknown.
    public let pixelWidth: Int
    public let pixelHeight: Int
    /// Burst / near-duplicate group (stub: capture-time proximity). Dense, ascending with `id`.
    public internal(set) var groupID: Int
    /// Seed for generated thumbnails of synthetic items.
    public let seed: UInt64
    public let engineImage: EngineImageReference?

    public init(id: Int, url: URL?, name: String, kind: PhotoKind, captureDate: Date,
                pixelWidth: Int, pixelHeight: Int, groupID: Int = 0, seed: UInt64 = 0,
                engineImage: EngineImageReference? = nil) {
        self.id = id
        self.url = url
        self.name = name
        self.kind = kind
        self.captureDate = captureDate
        self.pixelWidth = pixelWidth
        self.pixelHeight = pixelHeight
        self.groupID = groupID
        self.seed = seed
        self.engineImage = engineImage
    }

    public var aspectRatio: Double {
        guard pixelWidth > 0, pixelHeight > 0 else { return 1.5 }
        return Double(pixelWidth) / Double(pixelHeight)
    }

    func with(id: Int, groupID: Int) -> PhotoItem {
        PhotoItem(id: id, url: url, name: name, kind: kind, captureDate: captureDate,
                  pixelWidth: pixelWidth, pixelHeight: pixelHeight, groupID: groupID, seed: seed, engineImage: engineImage)
    }
}
