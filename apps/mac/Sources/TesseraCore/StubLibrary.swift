import Foundation
import ImageIO

/// Stand-in for the Rust index until `crates/index` + UniFFI land. Lists the JPEG / RAW files of one
/// folder (non-recursive), reads capture time and size from their metadata via ImageIO, sorts by
/// capture time and assigns stub groups. Can also generate N synthetic items for performance testing.
public final class StubLibrary: Sendable {
    public let title: String
    public let folder: URL?
    public let items: [PhotoItem]
    public let groups: [Range<Int>]
    /// Immediate subfolders of `folder` (shown in the sidebar).
    public let subfolders: [URL]
    public let scanDuration: TimeInterval

    public static let rawExtensions: Set<String> = [
        "3fr", "ari", "arw", "cr2", "cr3", "crw", "dcr", "dng", "erf", "fff", "iiq", "k25", "kdc",
        "mef", "mos", "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f",
    ]
    public static let jpegExtensions: Set<String> = ["jpg", "jpeg", "jpe"]
    public static let otherExtensions: [String: PhotoKind] = ["png": .png, "heic": .heif, "heif": .heif, "hif": .heif, "tif": .tiff, "tiff": .tiff]

    public static func kind(forExtension ext: String) -> PhotoKind? {
        let e = ext.lowercased()
        if rawExtensions.contains(e) { return .raw }
        if jpegExtensions.contains(e) { return .jpeg }
        return otherExtensions[e]
    }

    public static let empty = StubLibrary(title: "No Folder", folder: nil, items: [], subfolders: [], scanDuration: 0)

    init(title: String, folder: URL?, items unsorted: [PhotoItem], subfolders: [URL], scanDuration: TimeInterval,
         maxGap: TimeInterval = CaptureGrouper.defaultMaxGap) {
        let sorted = unsorted.sorted {
            $0.captureDate != $1.captureDate ? $0.captureDate < $1.captureDate
                : $0.name.localizedStandardCompare($1.name) == .orderedAscending
        }
        let groups = CaptureGrouper.groups(forSortedDates: sorted.map(\.captureDate), maxGap: maxGap)
        var items = [PhotoItem]()
        items.reserveCapacity(sorted.count)
        for (g, range) in groups.enumerated() {
            for i in range { items.append(sorted[i].with(id: i, groupID: g)) }
        }
        self.title = title
        self.folder = folder
        self.items = items
        self.groups = groups
        self.subfolders = subfolders
        self.scanDuration = scanDuration
    }

    // MARK: Folder scan

    public enum ScanError: LocalizedError {
        case unreadable(URL, Error)
        public var errorDescription: String? {
            switch self {
            case .unreadable(let url, let err): "Could not read \(url.path): \(err.localizedDescription)"
            }
        }
    }

    /// Lists supported files in `folder` and reads their metadata in parallel. Runs off the main actor.
    public static func scan(folder: URL) throws -> StubLibrary {
        let start = Date()
        let fm = FileManager.default
        let keys: [URLResourceKey] = [.isDirectoryKey, .isHiddenKey, .contentModificationDateKey]
        let entries: [URL]
        do {
            entries = try fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles])
        } catch {
            throw ScanError.unreadable(folder, error)
        }

        var files: [(URL, PhotoKind)] = []
        var subfolders: [URL] = []
        for url in entries {
            let values = try? url.resourceValues(forKeys: Set(keys))
            if values?.isDirectory == true {
                subfolders.append(url)
            } else if let kind = kind(forExtension: url.pathExtension) {
                files.append((url, kind))
            }
        }
        subfolders.sort { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }

        // Metadata read is I/O + header parse only (no decode); parallelise across cores.
        let found = files
        let count = found.count
        let results = UnsafeMutableBufferPointer<PhotoItem?>.allocate(capacity: count)
        results.initialize(repeating: nil)
        defer { results.deallocate() }
        nonisolated(unsafe) let out = results
        DispatchQueue.concurrentPerform(iterations: count) { i in
            let (url, kind) = found[i]
            out[i] = MetadataReader.item(url: url, kind: kind)
        }
        let items = results.compactMap { $0 }
        return StubLibrary(title: folder.lastPathComponent, folder: folder, items: items,
                           subfolders: subfolders, scanDuration: Date().timeIntervalSince(start))
    }

    // MARK: Synthetic items

    /// Generates `count` synthetic items shot in bursts (1–9 frames, 0.2–1.5 s apart) separated by
    /// 3–180 s pauses, so the stub grouper produces realistic groups. Deterministic for a seed.
    public static func synthetic(count: Int, seed: UInt64 = 0x5EED) -> StubLibrary {
        let start = Date()
        var rng = SplitMix64(seed: seed)
        var t = Date(timeIntervalSince1970: 1_750_000_000)
        var items = [PhotoItem]()
        items.reserveCapacity(count)
        var remainingInBurst = 0
        for i in 0..<count {
            if remainingInBurst == 0 {
                remainingInBurst = 1 + Int(rng.next() % 9)
                t += 3 + Double(rng.next() % 177)
            } else {
                t += 0.2 + Double(rng.next() % 13) / 10
            }
            remainingInBurst -= 1
            let portrait = rng.next() % 5 == 0
            items.append(PhotoItem(
                id: i, url: nil, name: String(format: "STUB_%05d.CR3", i + 1), kind: .synthetic,
                captureDate: t, pixelWidth: portrait ? 4000 : 6000, pixelHeight: portrait ? 6000 : 4000,
                seed: rng.next()))
        }
        return StubLibrary(title: "\(count.formatted()) Stub Items", folder: nil, items: items,
                           subfolders: [], scanDuration: Date().timeIntervalSince(start))
    }
}

/// Deterministic PRNG for stub data.
public struct SplitMix64: RandomNumberGenerator, Sendable {
    private var state: UInt64
    public init(seed: UInt64) { state = seed }
    public mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
}

/// Reads capture time and oriented pixel size from file headers (no pixel decode).
enum MetadataReader {
    private static let calendar: Calendar = {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = .current
        return c
    }()

    static func item(url: URL, kind: PhotoKind) -> PhotoItem {
        var date: Date?
        var width = 0
        var height = 0
        let opts = [kCGImageSourceShouldCache: false] as CFDictionary
        if let src = CGImageSourceCreateWithURL(url as CFURL, opts),
           let props = CGImageSourceCopyPropertiesAtIndex(src, 0, opts) as? [CFString: Any] {
            width = props[kCGImagePropertyPixelWidth] as? Int ?? 0
            height = props[kCGImagePropertyPixelHeight] as? Int ?? 0
            if let orientation = props[kCGImagePropertyOrientation] as? Int, (5...8).contains(orientation) {
                swap(&width, &height)
            }
            if let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any] {
                let sub = exif[kCGImagePropertyExifSubsecTimeOriginal] as? String
                date = parseExifDate(exif[kCGImagePropertyExifDateTimeOriginal] as? String, subsec: sub)
                    ?? parseExifDate(exif[kCGImagePropertyExifDateTimeDigitized] as? String, subsec: nil)
            }
            if date == nil, let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any] {
                date = parseExifDate(tiff[kCGImagePropertyTIFFDateTime] as? String, subsec: nil)
            }
        }
        if date == nil {
            date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        }
        return PhotoItem(id: 0, url: url, name: url.lastPathComponent, kind: kind,
                         captureDate: date ?? .distantPast, pixelWidth: width, pixelHeight: height)
    }

    /// Parses "YYYY:MM:DD HH:MM:SS" (EXIF) in the local time zone.
    static func parseExifDate(_ s: String?, subsec: String?) -> Date? {
        guard let s, s.utf8.count >= 19 else { return nil }
        let parts = s.split(whereSeparator: { $0 == ":" || $0 == " " || $0 == "-" || $0 == "T" }).compactMap { Int($0) }
        guard parts.count >= 6, parts[0] > 0 else { return nil }
        let comps = DateComponents(year: parts[0], month: parts[1], day: parts[2],
                                   hour: parts[3], minute: parts[4], second: parts[5])
        guard var d = calendar.date(from: comps) else { return nil }
        if let subsec, let frac = Double("0." + subsec.trimmingCharacters(in: .whitespaces)) { d += frac }
        return d
    }
}
