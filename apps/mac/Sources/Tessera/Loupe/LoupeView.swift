import AppKit
import TesseraCore
import SwiftUI

struct LoupeView: NSViewRepresentable {
    let model: AppModel

    func makeCoordinator() -> LoupeController { LoupeController(model: model) }
    func makeNSView(context: Context) -> MetalLoupeView { context.coordinator.view }
    func updateNSView(_ nsView: MetalLoupeView, context: Context) {}
}

/// Feeds the loupe: instant first paint from the cached grid thumbnail, then the embedded preview,
/// and prefetches the neighbours (docs/08 §2 "prefetch next/previous").
@MainActor
final class LoupeController: LibraryObserver {
    let model: AppModel
    let view = MetalLoupeView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
    private var shownID: Int?
    private var request: PreviewRequest?
    private var prefetch: [PreviewRequest] = []

    deinit {
        request?.cancel()
        prefetch.forEach { $0.cancel() }
    }

    init(model: AppModel) {
        self.model = model
        view.onColorInfoChange = { [weak model] info in
            if model?.loupeInfo != info { model?.loupeInfo = info }
        }
        model.addObserver(self)
    }

    func libraryDidReload() {
        shownID = nil
        view.present(image: nil, isFinal: true)
        selectionDidChange(scrollToFocus: false)
    }

    func itemsDidChange(_ positions: IndexSet) {}

    func selectionDidChange(scrollToFocus: Bool) {
        guard model.viewMode == .loupe, let f = model.focus, f < model.visibleCount else { return }
        let item = model.item(at: f)
        guard item.id != shownID else { return }
        shownID = item.id
        view.exposure = Float(model.adjustment(.exposure, for: item.id))
        request?.cancel()
        prefetch.forEach { $0.cancel() }
        prefetch.removeAll()

        if let preview = model.loader.cached(item, tier: .preview) {
            view.present(image: preview, isFinal: true)
        } else {
            view.present(image: model.loader.cached(item, tier: .thumbnail), isFinal: false)
            let id = item.id
            request = model.loader.request(item, tier: .preview, priority: .veryHigh) { [weak self] image in
                guard let self, self.shownID == id else { return }
                self.view.present(image: image, isFinal: true)
            }
        }
        for n in [f + 1, f - 1, f + 2] where n >= 0 && n < model.visibleCount {
            if let r = model.loader.request(model.item(at: n), tier: .preview, priority: .low, completion: { _ in }) {
                prefetch.append(r)
            }
        }
    }

    func adjustmentsDidChange(itemID: Int) {
        guard itemID == shownID else { return }
        view.exposure = Float(model.adjustment(.exposure, for: itemID))
    }
}
