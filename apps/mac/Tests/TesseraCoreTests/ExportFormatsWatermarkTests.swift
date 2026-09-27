import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Export dialog M2-46: AVIF / JPEG XL / DNG, the JPEG size limit and watermarks. Settings JSON the
/// engine accepts verbatim, presets that keep the new fields while M2-20 presets load unchanged,
/// and real small exports through `export_batch` checked with ImageIO.
final class ExportFormatsWatermarkTests: XCTestCase {
    private var scratch: URL!

    override func setUpWithError() throws {
        scratch = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-m2-46-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: scratch)
    }

    private func object(_ json: String) throws -> [String: Any] {
        try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }

    private var textMark: ExportWatermark {
        var m = ExportWatermark(kind: .text)
        m.text = "© Tessera"
        m.font = ExportWatermark.defaultFontPath
        m.size = 0.25
        m.color = [1, 0.5, 0.25]
        m.opacity = 1
        m.anchor = .bottomRight
        m.inset = 0.05
        m.rotation = -15
        return m
    }

    // MARK: Settings → FFI JSON

    func testNewFieldsEncodeAsTheEngineExpects() throws {
        var s = ExportSettings()
        s.format = .avif
        s.quality = 72
        s.bitDepth = 10
        s.avifSpeed = 4
        s.colorSpace = .displayP3
        s.watermark = textMark
        s.destination = "/tmp/out"
        let json = try object(s.json)
        XCTAssertEqual(json["format"] as? String, "avif")
        XCTAssertEqual(json["avif_speed"] as? Int, 4)
        XCTAssertEqual(json["bit_depth"] as? Int, 10)
        XCTAssertNil(json["max_file_bytes"], "no limit is omitted (the engine's default)")
        let mark = try XCTUnwrap(json["watermark"] as? [String: Any])
        XCTAssertEqual(Set(mark.keys), ["kind", "text", "font", "size", "color", "opacity", "anchor", "inset", "rotation"],
                       "only the text fields: the engine rejects unknown keys")
        XCTAssertEqual(mark["kind"] as? String, "text")
        XCTAssertEqual(mark["anchor"] as? String, "bottom_right")
        // The engine accepts exactly this and Swift reads back what it writes.
        XCTAssertEqual(try ExportSettings(json: try normalizeExportSettings(json: s.json)), s)

        var g = ExportSettings()
        g.format = .jpeg
        g.maxFileKilobytes = 350
        var graphic = ExportWatermark(kind: .graphic)
        graphic.path = "/Users/someone/logo.png"
        graphic.scale = 0.2
        graphic.anchor = .topLeft
        g.watermark = graphic
        let gj = try object(g.json)
        XCTAssertEqual(gj["max_file_bytes"] as? Int, 350_000)
        XCTAssertEqual(Set(try XCTUnwrap(gj["watermark"] as? [String: Any]).keys),
                       ["kind", "path", "scale", "opacity", "anchor", "inset"])
        XCTAssertEqual(try ExportSettings(json: try normalizeExportSettings(json: g.json)), g)

        for (format, depth) in [(ExportSettings.OutputFormat.jpegXl, 16), (.dng, 32), (.tiff, 16), (.avif, 12)] {
            var f = ExportSettings()
            f.format = format
            f.bitDepth = depth
            let normalized = try normalizeExportSettings(json: f.json)
            XCTAssertEqual(try ExportSettings(json: normalized), f, format.rawValue)
            XCTAssertTrue(normalized.contains("\"\(format.rawValue)\""))
        }
    }

    /// The sheet's normalisation leaves only combinations the engine accepts, and says why the
    /// others are unavailable.
    func testFormatSwitchesStayValidForTheEngine() throws {
        var s = ExportSettings()
        s.maxFileKilobytes = 200
        s.watermark = textMark
        s.colorSpace = .prophoto
        s.bitDepth = 16
        for format in ExportSettings.OutputFormat.allCases {
            var f = s
            f.format = format
            f.normalizeForFormat()
            XCTAssertTrue(format.bitDepths.contains(f.bitDepth), format.rawValue)
            XCTAssertNoThrow(try normalizeExportSettings(json: f.json), format.rawValue)
            XCTAssertEqual(f.maxFileBytes != nil, format == .jpeg, format.rawValue)
            XCTAssertEqual(f.watermark != nil, format != .dng, format.rawValue)
            XCTAssertEqual(f.watermarkUnavailableReason != nil, format == .dng)
            XCTAssertEqual(f.colorSpaceLockedReason != nil, format == .jpegXl || format == .dng)
        }
        var jxl = s
        jxl.format = .jpegXl
        jxl.normalizeForFormat()
        XCTAssertEqual(jxl.colorSpace, .srgb)
        XCTAssertEqual(jxl.bitDepth, 16, "JPEG XL keeps 16-bit")
        // What normalisation prevents, the engine refuses.
        var bad = ExportSettings()
        bad.format = .png
        bad.maxFileBytes = 100_000
        XCTAssertThrowsError(try normalizeExportSettings(json: bad.json))
        bad = ExportSettings()
        bad.format = .jpegXl
        bad.colorSpace = .displayP3
        XCTAssertThrowsError(try normalizeExportSettings(json: bad.json))
        bad = ExportSettings()
        bad.format = .avif
        bad.bitDepth = 16
        XCTAssertThrowsError(try normalizeExportSettings(json: bad.json))
        var mark = textMark
        mark.size = 0
        bad = ExportSettings()
        bad.watermark = mark
        XCTAssertThrowsError(try normalizeExportSettings(json: bad.json))

        XCTAssertEqual(ExportSettings.OutputFormat.jpegXl.fileExtension, "jxl")
        XCTAssertEqual(ExportSettings.OutputFormat.avif.fileExtension, "avif")
        XCTAssertEqual(ExportSettings.OutputFormat.dng.fileExtension, "dng")
        XCTAssertFalse(ExportSettings.OutputFormat.jpegXl.usesQuality, "JPEG XL is lossless only")
        XCTAssertEqual(ExportSettings.FileFormat.allCases.count, 3, "Export Flat keeps its three codecs")
        var summary = ExportSettings()
        summary.maxFileKilobytes = 500
        summary.watermark = textMark
        XCTAssertEqual(summary.summary, "Full size · JPEG 90 ≤ 500 KB · sRGB · 72 dpi · Text watermark")
        summary = ExportSettings()
        summary.format = .dng
        summary.bitDepth = 32
        XCTAssertEqual(summary.summary, "Full size · DNG linear float · Linear Rec. 2020 · 72 dpi")
    }

    func testWatermarkProblemsAndPlacement() throws {
        XCTAssertFalse(ExportWatermark.installedFonts.isEmpty, "single-face fonts are installed")
        XCTAssertTrue(ExportWatermark.installedFonts.allSatisfy { ["ttf", "otf"].contains(URL(fileURLWithPath: $0.path).pathExtension.lowercased()) })
        XCTAssertNil(textMark.problem)
        var m = textMark
        m.text = "  "
        XCTAssertEqual(m.problem, "Type the watermark text")
        m = textMark
        m.font = "/nonexistent/font.ttf"
        XCTAssertNotNil(m.problem)
        m = ExportWatermark(kind: .graphic)
        XCTAssertEqual(m.problem, "Choose a PNG for the watermark")
        // Placement mirrors apply_watermark: inset from the short edge, centred on the middle axis.
        m.anchor = .bottomRight
        m.inset = 0.1
        let o = m.origin(markWidth: 30, markHeight: 10, width: 300, height: 200)
        XCTAssertEqual(o.x, 300 - 30 - 20, accuracy: 1e-9)
        XCTAssertEqual(o.y, 200 - 10 - 20, accuracy: 1e-9)
        m.anchor = .center
        let c = m.origin(markWidth: 30, markHeight: 10, width: 300, height: 200)
        XCTAssertEqual(c.x, 135, accuracy: 1e-9)
        XCTAssertEqual(c.y, 95, accuracy: 1e-9)
        XCTAssertTrue(ExportWatermark.Anchor.topRight.axes == (2, 0))
        XCTAssertTrue(ExportWatermark.Anchor.bottomLeft.axes == (0, 2))
        // Switching kinds keeps both kinds' fields in memory; JSON carries the chosen kind only.
        var both = textMark
        both.path = "/x.png"
        both.kind = .graphic
        let back = try JSONDecoder().decode(ExportWatermark.self, from: JSONEncoder().encode(both))
        XCTAssertEqual(back.path, "/x.png")
        XCTAssertEqual(back.text, ExportWatermark().text, "text fields are not written for a graphic")
    }

    // MARK: Presets

    func testPresetsKeepNewFieldsAndOldPresetsLoadUnchanged() throws {
        let support = scratch.appendingPathComponent("support").path
        var store = ExportPresetStore(engine: try Engine.open(appSupportDir: support))
        let shipped = try store.list()
        XCTAssertEqual(shipped.count, 4)
        XCTAssertTrue(shipped.allSatisfy { $0.settings.watermark == nil && $0.settings.maxFileBytes == nil && $0.settings.avifSpeed == 6 })

        // An M2-20 preset file (no avif_speed / max_file_bytes / watermark keys), as older builds wrote it.
        let legacy = #"""
        {"name":"Old proofs","settings":{"format":"jpeg","quality":77,"bit_depth":8,"color_space":"display_p3",
        "resize":{"mode":"long_edge","unit":"px","long_edge":1600,"width":2048,"height":2048,"percent":100},
        "dpi":240,"sharpening":"matte","metadata":"copyright","naming":"{name}-{seq}","upscale":1,"destination":"",
        "on_conflict":"skip","open_in_finder":true}}
        """#
        try legacy.write(toFile: support + "/ExportPresets/Old proofs.json", atomically: true, encoding: .utf8)
        var expected = ExportSettings()
        expected.quality = 77
        expected.colorSpace = .displayP3
        expected.resize.mode = .longEdge
        expected.resize.longEdge = 1600
        expected.dpi = 240
        expected.sharpening = .matte
        expected.metadata = .copyright
        expected.naming = "{name}-{seq}"
        expected.onConflict = .skip
        expected.openInFinder = true
        XCTAssertEqual(try store.list().first { $0.name == "Old proofs" }?.settings, expected)
        // Swift-side persisted settings (UserDefaults) from before M2-46 decode the same way.
        let oldDefaults = #"{"bit_depth":16,"color_space":"prophoto","format":"tiff","quality":90}"#
        let decoded = try ExportSettings(json: oldDefaults)
        XCTAssertEqual(decoded.format, .tiff)
        XCTAssertNil(decoded.watermark)
        XCTAssertNil(decoded.maxFileBytes)

        // New fields persist in user presets across a relaunch.
        var avif = ExportSettings()
        avif.format = .avif
        avif.quality = 64
        avif.bitDepth = 12
        avif.avifSpeed = 3
        avif.watermark = textMark
        try store.save("AVIF marked", avif)
        var limited = ExportSettings()
        limited.maxFileKilobytes = 450
        var graphic = ExportWatermark(kind: .graphic)
        graphic.path = "/Users/someone/logo.png"
        graphic.opacity = 0.35
        graphic.anchor = .top
        limited.watermark = graphic
        try store.save("Web ≤ 450 KB", limited)
        var dng = ExportSettings()
        dng.format = .dng
        dng.bitDepth = 32
        try store.save("Developed DNG", dng)
        var jxl = ExportSettings()
        jxl.format = .jpegXl
        jxl.bitDepth = 16
        try store.save("Archive JXL", jxl)
        // Re-saving the legacy preset keeps it equal.
        try store.save("Old proofs", expected)

        store = ExportPresetStore(engine: try Engine.open(appSupportDir: support))
        let reloaded = Dictionary(uniqueKeysWithValues: try store.list().map { ($0.name, $0.settings) })
        XCTAssertEqual(reloaded["AVIF marked"], avif)
        XCTAssertEqual(reloaded["Web ≤ 450 KB"], limited)
        XCTAssertEqual(reloaded["Developed DNG"], dng)
        XCTAssertEqual(reloaded["Archive JXL"], jxl)
        XCTAssertEqual(reloaded["Old proofs"], expected)
    }

    // MARK: Real exports through the engine

    /// A folder with one noisy 480 × 320 JPEG, scanned by the engine.
    private func scratchPhoto() throws -> EngineImageReference {
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let w = 480, h = 320
        var rng = SystemRandomNumberGenerator()
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                // Dark gradient plus noise: large as a JPEG, and a white watermark shows on it.
                pixels[i] = UInt8(clamping: x / 8 + Int.random(in: 0..<60, using: &rng))
                pixels[i + 1] = UInt8(clamping: y / 8 + Int.random(in: 0..<60, using: &rng))
                pixels[i + 2] = UInt8(clamping: 30 + Int.random(in: 0..<60, using: &rng))
                pixels[i + 3] = 255
            }
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let url = folder.appendingPathComponent("noise.jpg")
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 1.0] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        let library = try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support"))
        return try XCTUnwrap(library.items.first?.engineImage)
    }

    private func export(_ ref: EngineImageReference, _ settings: ExportSettings, into name: String) throws -> URL {
        var s = settings
        s.destination = scratch.appendingPathComponent(name).path
        let report = try ref.engine.exportBatch(target: .images(imageIds: [ref.imageID]), settingsJson: s.json,
                                                listener: nil, cancel: nil)
        XCTAssertEqual(report.exported, 1, "\(name): \(report.items.map { $0.error ?? "" })")
        return URL(fileURLWithPath: try XCTUnwrap(report.items.first?.outputPath, name))
    }

    private func decode(_ url: URL) -> CGImage? {
        CGImageSourceCreateWithURL(url as CFURL, nil).flatMap { CGImageSourceCreateImageAtIndex($0, 0, nil) }
    }

    private func rgba(_ image: CGImage) throws -> [UInt8] {
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        pixels.withUnsafeMutableBytes { buf in
            let ctx = CGContext(data: buf.baseAddress, width: image.width, height: image.height, bitsPerComponent: 8,
                                bytesPerRow: image.width * 4, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            ctx?.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        }
        return pixels
    }

    private func imageIOReads(_ type: UTType) -> Bool {
        ((CGImageSourceCopyTypeIdentifiers() as? [String]) ?? []).contains(type.identifier)
    }

    func testRealExportsAVIFJXLAndDNG() throws {
        let ref = try scratchPhoto()

        var avif = ExportSettings()
        avif.format = .avif
        avif.quality = 70
        avif.bitDepth = 10
        avif.avifSpeed = 10
        let avifURL = try export(ref, avif, into: "avif")
        XCTAssertEqual(avifURL.pathExtension, "avif")
        let avifImage = try XCTUnwrap(decode(avifURL), "ImageIO decodes the AVIF")
        XCTAssertEqual(avifImage.width, 480)
        XCTAssertEqual(avifImage.height, 320)

        var jxl = ExportSettings()
        jxl.format = .jpegXl
        jxl.bitDepth = 8
        let jxlURL = try export(ref, jxl, into: "jxl")
        XCTAssertEqual(jxlURL.pathExtension, "jxl")
        let head = try [UInt8](Data(contentsOf: jxlURL).prefix(12))
        XCTAssertTrue(head.starts(with: [0xFF, 0x0A]) || head.starts(with: [0, 0, 0, 0x0C, 0x4A, 0x58, 0x4C, 0x20]),
                      "JPEG XL signature: \(head)")
        if let jxlType = UTType("public.jpeg-xl"), imageIOReads(jxlType) {
            let image = try XCTUnwrap(decode(jxlURL), "ImageIO decodes the JPEG XL")
            XCTAssertEqual(image.width, 480)
        }

        var dng = ExportSettings()
        dng.format = .dng
        dng.bitDepth = 32
        let dngURL = try export(ref, dng, into: "dng")
        XCTAssertEqual(dngURL.pathExtension, "dng")
        let tiffHead = try [UInt8](Data(contentsOf: dngURL).prefix(4))
        XCTAssertTrue(tiffHead == [0x49, 0x49, 0x2A, 0x00] || tiffHead == [0x4D, 0x4D, 0x00, 0x2A], "DNG is TIFF-based: \(tiffHead)")
        // Apple ImageIO does not decode this linear float DNG (REPORT.md), so read IFD0 directly:
        // DNGVersion 1.4 and the developed picture's size.
        let tags = try ifd0(Data(contentsOf: dngURL))
        XCTAssertEqual(tags[50706], 0x0000_0401, "DNGVersion 1.4.0.0")
        XCTAssertEqual(tags[256], 480)
        XCTAssertEqual(tags[257], 320)
        XCTAssertEqual(tags[258], 32, "32-bit samples")
        XCTAssertEqual(tags[339], 3, "SampleFormat: IEEE float")
    }

    /// IFD0 of a little-endian TIFF: tag → first value (SHORT, single LONG, BYTE ×4; else the count).
    private func ifd0(_ d: Data) throws -> [Int: UInt32] {
        let b = [UInt8](d)
        func u16(_ o: Int) -> Int { Int(b[o]) | Int(b[o + 1]) << 8 }
        func u32(_ o: Int) -> UInt32 { UInt32(b[o]) | UInt32(b[o + 1]) << 8 | UInt32(b[o + 2]) << 16 | UInt32(b[o + 3]) << 24 }
        XCTAssertEqual(Array(b.prefix(2)), [0x49, 0x49])
        let ifd = Int(u32(4))
        var tags: [Int: UInt32] = [:]
        for i in 0..<u16(ifd) {
            let e = ifd + 2 + i * 12
            let type = u16(e + 2), count = u32(e + 4)
            switch type {
            case 3 where count <= 2: tags[u16(e)] = UInt32(u16(e + 8))
            case 3: tags[u16(e)] = UInt32(u16(Int(u32(e + 8))))
            case 4 where count == 1: tags[u16(e)] = u32(e + 8)
            case 1 where count == 4: tags[u16(e)] = u32(e + 8)
            default: tags[u16(e)] = count
            }
        }
        return tags
    }

    func testJPEGSizeLimitConverges() throws {
        let ref = try scratchPhoto()
        var full = ExportSettings()
        full.quality = 100
        full.metadata = .none
        let unlimited = try export(ref, full, into: "unlimited")
        let unlimitedBytes = try XCTUnwrap(try unlimited.resourceValues(forKeys: [.fileSizeKey]).fileSize)
        var limited = full
        let budget = unlimitedBytes / 3
        limited.maxFileBytes = budget
        let url = try export(ref, limited, into: "limited")
        let bytes = try XCTUnwrap(try url.resourceValues(forKeys: [.fileSizeKey]).fileSize)
        XCTAssertLessThanOrEqual(bytes, budget, "fits the budget (\(bytes) of \(budget); unlimited \(unlimitedBytes))")
        XCTAssertGreaterThan(bytes, budget / 4, "the search keeps as much quality as fits")
        XCTAssertEqual(try XCTUnwrap(decode(url)).width, 480)
        // A budget no quality can meet fails the photo and writes nothing.
        var impossible = full
        impossible.maxFileBytes = 200
        impossible.destination = scratch.appendingPathComponent("impossible").path
        let report = try ref.engine.exportBatch(target: .images(imageIds: [ref.imageID]), settingsJson: impossible.json,
                                                listener: nil, cancel: nil)
        XCTAssertEqual(report.exported, 0)
        XCTAssertEqual(report.failed, 1)
    }

    func testWatermarksAreBurnedIn() throws {
        let ref = try scratchPhoto()
        var plain = ExportSettings()
        plain.format = .png
        plain.metadata = .none
        let base = try rgba(try XCTUnwrap(decode(try export(ref, plain, into: "plain"))))

        var text = plain
        var mark = textMark
        mark.color = [1, 1, 1]
        mark.rotation = 0
        mark.text = "WWWW"
        text.watermark = mark
        let marked = try rgba(try XCTUnwrap(decode(try export(ref, text, into: "text"))))
        // Bottom-right quadrant brightens (white text), top-left is untouched.
        func mean(_ p: [UInt8], _ xs: Range<Int>, _ ys: Range<Int>) -> Double {
            var sum = 0
            for y in ys { for x in xs { let i = (y * 480 + x) * 4; sum += Int(p[i]) + Int(p[i + 1]) + Int(p[i + 2]) } }
            return Double(sum) / Double(xs.count * ys.count * 3)
        }
        XCTAssertGreaterThan(mean(marked, 240..<480, 160..<320), mean(base, 240..<480, 160..<320) + 20)
        XCTAssertEqual(mean(marked, 0..<200, 0..<120), mean(base, 0..<200, 0..<120), accuracy: 0.5)

        // A graphic: a solid red PNG, centred at half the short edge, fully opaque.
        let png = scratch.appendingPathComponent("logo.png")
        let red = [UInt8](repeating: 0, count: 16 * 16 * 4).enumerated().map { $0.offset % 4 == 0 || $0.offset % 4 == 3 ? 255 : 0 }
        let image = try XCTUnwrap(CGImage(width: 16, height: 16, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 64,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                                          provider: try XCTUnwrap(CGDataProvider(data: Data(red.map(UInt8.init)) as CFData)),
                                          decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(png as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        var graphic = ExportWatermark(kind: .graphic)
        graphic.path = png.path
        graphic.scale = 0.5
        graphic.opacity = 1
        graphic.anchor = .center
        XCTAssertNil(graphic.problem)
        var g = plain
        g.watermark = graphic
        let stamped = try rgba(try XCTUnwrap(decode(try export(ref, g, into: "graphic"))))
        let centre = (160 * 480 + 240) * 4
        XCTAssertGreaterThan(stamped[centre], 230, "red channel at the centre")
        XCTAssertLessThan(stamped[centre + 1], 30)
        XCTAssertLessThan(stamped[centre + 2], 30)
        let corner = (10 * 480 + 10) * 4
        XCTAssertEqual(stamped[corner], base[corner], "outside the graphic nothing changes")

        // DNG refuses watermarks (the sheet disables them with this reason).
        var dng = g
        dng.format = .dng
        dng.bitDepth = 32
        dng.destination = scratch.appendingPathComponent("dng-marked").path
        // Either the whole batch or the one photo fails; nothing is written.
        let report = try? ref.engine.exportBatch(target: .images(imageIds: [ref.imageID]), settingsJson: dng.json,
                                                 listener: nil, cancel: nil)
        XCTAssertEqual(report?.exported ?? 0, 0)
        XCTAssertNotNil(dng.watermarkUnavailableReason)
    }
}
