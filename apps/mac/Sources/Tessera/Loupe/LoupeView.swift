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
/// then the engine's develop frames once the photo's session is open; prefetches the neighbours
/// (docs/08 §2 "prefetch next/previous").
@MainActor
final class LoupeController: LibraryObserver {
    let model: AppModel
    let view = MetalLoupeView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
    private var shownID: Int?
    /// The engine has presented a frame for `shownID`; late previews must not replace it.
    private var engineShown = false
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
        DevelopTools.shared.onLoupeToolChange = { [weak view] in view?.toolOverlay.toolsChanged() }
        MaskTools.shared.onLoupeChange = { [weak view] in
            view?.toolOverlay.toolsChanged()
            view?.render()
        }
        MaskTools.shared.onOverlayFrame = { [weak view] f in view?.present(maskOverlay: f) }
        SoftProof.shared.onChange = { [weak view] in
            let proof = SoftProof.shared
            view?.setSoftProof(proof.lut, warning: proof.warningRGB)
        }
    }

    func libraryDidReload() {
        shownID = nil
        engineShown = false
        view.attach(develop: nil)
        view.present(image: nil, isFinal: true)
        selectionDidChange(scrollToFocus: false)
    }

    /// In place: the shown photo keeps its frame (and develop session) under its new id.
    func libraryDidUpdate(_ change: VisibleChange) {
        shownID = shownID.flatMap(change.remap)
        if shownID == nil { engineShown = false }
        selectionDidChange(scrollToFocus: false)
    }

    func itemsDidChange(_ positions: IndexSet) {}

    func selectionDidChange(scrollToFocus: Bool) {
        guard model.viewMode == .loupe, let f = model.focus, f < model.visibleCount else { return }
        let item = model.item(at: f)
        guard item.id != shownID else {
            developDidChange()
            return
        }
        shownID = item.id
        engineShown = false
        view.attach(develop: nil)
        request?.cancel()
        prefetch.forEach { $0.cancel() }
        prefetch.removeAll()

        if let preview = model.loader.cached(item, tier: .preview) {
            view.present(image: preview, isFinal: true)
        } else {
            view.present(image: model.loader.cached(item, tier: .thumbnail), isFinal: false)
            let id = item.id
            request = model.loader.request(item, tier: .preview, priority: .veryHigh) { [weak self] image in
                guard let self, self.shownID == id, !self.engineShown else { return }
                self.view.present(image: image, isFinal: true)
            }
        }
        for n in [f + 1, f - 1, f + 2] where n >= 0 && n < model.visibleCount {
            if let r = model.loader.request(model.item(at: n), tier: .preview, priority: .low, completion: { _ in }) {
                prefetch.append(r)
            }
        }
        model.openDevelop(for: item)
    }

    /// The model's develop session changed (opened, closed): attach it if it is ours.
    func developDidChange() {
        if SoftProof.shared.develop !== model.develop { SoftProof.shared.attach(model.develop) }
        guard model.viewMode == .loupe, let develop = model.develop, develop.itemID == shownID else { return }
        view.attach(develop: develop)
    }

    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
        guard controller.itemID == shownID else { return }
        if view.develop !== controller { view.attach(develop: controller) }
        engineShown = true
        view.present(developFrame: frame, from: controller)
    }
}
