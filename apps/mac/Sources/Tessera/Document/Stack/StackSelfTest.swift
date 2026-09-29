import AppKit
import TesseraCore

/// Test aid (WP B5-19): with `TESSERA_STACK_SELFTEST=<dir>` in the environment, writes two overlapping
/// PNG crops of a synthetic scene into `<dir>`, runs File ▸ Automate ▸ Photomerge on them through
/// `DocumentStack` (a new document), checks two named masked layers and one history node, undoes and
/// redoes it, saves `<dir>/StackSelfTest.tessera-doc`, then runs Auto-Blend Layers again on the two
/// layers. It never brings a window to the front. Checks print `stack-selftest: check <name> ok|FAIL`,
/// and the run ends with `stack-selftest: done, <n> failure(s)`.
@MainActor
final class StackSelfTest {
    private let model: AppModel
    private let dir: URL
    private var failures = 0
    private static var started = false

    static func startIfRequested(_ model: AppModel) {
        guard !started, let path = ProcessInfo.processInfo.environment["TESSERA_STACK_SELFTEST"] else { return }
        started = true
        let test = StackSelfTest(model: model, dir: URL(fileURLWithPath: path))
        Task { @MainActor in await test.run() }
    }

    private init(model: AppModel, dir: URL) { self.model = model; self.dir = dir }

    private func log(_ s: String) { FileHandle.standardError.write(Data("stack-selftest: \(s)\n".utf8)) }

    private func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
        if !ok { failures += 1 }
        log("check \(name) " + (ok ? "ok" : "FAIL \(detail())"))
    }

    private func wait(_ timeout: Double, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            try? await Task.sleep(for: .milliseconds(50))
        }
        return true
    }

    /// A 240 × 180 crop of a textured scene starting at `offset`.
    private func writeCrop(_ name: String, offset: Double) -> URL? {
        let (w, h) = (240, 180)
        var bytes = [UInt8](repeating: 255, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let u = Double(x) + offset, v = Double(y)
                let p = 0.5 + 0.12 * sin(u * 0.17 + v * 0.09) + 0.1 * cos(u * 0.07 - v * 0.21)
                    + 0.12 * sin(sin(u * 0.039) * 7 + cos(v * 0.051) * 9)
                let i = (y * w + x) * 4
                bytes[i] = UInt8(max(0, min(255, p * 255)))
                bytes[i + 1] = UInt8(max(0, min(255, p * 0.8 * 255)))
                bytes[i + 2] = UInt8(max(0, min(255, p * 0.6 * 255)))
            }
        }
        guard let ctx = CGContext(data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue),
              let image = ctx.makeImage() else { return nil }
        let url = dir.appendingPathComponent("\(name).png")
        guard let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, image, nil)
        return CGImageDestinationFinalize(dest) ? url : nil
    }

    private func run() async {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let stack = DocumentStack.shared
        guard let a = writeCrop("stack-left", offset: 0), let b = writeCrop("stack-right", offset: 140) else {
            check("write crops", false); log("done, \(failures) failure(s)"); return
        }
        let before = model.documents.documents.count
        stack.photomerge = PhotomergeForm(sources: [.file(a), .file(b)])
        stack.photomerge.layout = .collage
        stack.runPhotomerge()
        check("busy while merging", stack.busy == "Photomerge")
        _ = await wait(120) { stack.busy == nil }
        check("new document", model.documents.documents.count == before + 1)
        guard let doc = model.documents.current else { log("done, \(failures + 1) failure(s)"); return }
        check("two named layers", doc.layers.map(\.name).sorted() == ["stack-left", "stack-right"], "\(doc.layers.map(\.name))")
        check("masks", doc.layers.allSatisfy(\.hasMask))
        check("one history node", doc.history.map(\.label) == ["Photomerge"], "\(doc.history.map(\.label))")
        check("canvas grew", doc.info.width >= 379, "\(doc.info.width)")
        doc.run("Undo") { try doc.backend.undo() }
        check("undo empties", doc.layers.isEmpty)
        doc.run("Redo") { try doc.backend.redo() }
        check("redo restores", doc.layers.count == 2)
        let path = dir.appendingPathComponent("StackSelfTest.tessera-doc").path
        do { try doc.backend.saveAs(path: path) } catch { check("save", false, "\(error)") }
        check("saved", FileManager.default.fileExists(atPath: path))
        doc.selection = doc.layers.map(\.id)
        check("blend enabled for aligned layers", stack.canBlend || model.viewMode != .document)
        log("done, \(failures) failure(s)")
    }
}
