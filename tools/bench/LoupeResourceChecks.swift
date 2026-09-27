import Foundation
import QuartzCore

@main struct LoupeResourceChecks {
    @MainActor static func main() async {
        let start = CACurrentMediaTime()
        guard let first = await LoupeRenderer.prepared() else { fatalError("Metal initialization failed") }
        let firstMs = (CACurrentMediaTime() - start) * 1000
        let warmStart = CACurrentMediaTime()
        guard let second = await LoupeRenderer.prepared() else { fatalError("Retained renderer missing") }
        let reuseMs = (CACurrentMediaTime() - warmStart) * 1000
        precondition(first === second)
        precondition(!first.createdOnMainThread)
        let result: [String: Any] = ["first_prepare_ms": firstMs, "retained_reuse_ms": reuseMs,
                                     "created_on_main": first.createdOnMainThread, "shared_identity": first === second,
                                     "shader_cache": "uncontrolled; no cache purge"]
        let data = try! JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    }
}
