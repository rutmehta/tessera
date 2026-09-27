import AppKit

/// Main-run-loop animation shared by document overlays. No timer retains its view.
/// Visibility notifications stop hidden work; the callback also rechecks eligibility
/// so a selection/ownership change cannot produce a stale animation tick.
@MainActor
class DocumentAnimatedOverlayView: NSView {
    private(set) var animationTimer: Timer?
    // Injectable unscheduled timer for deterministic lifecycle tests.
    var makeAnimationTimer: (@escaping @Sendable (Timer) -> Void) -> Timer = {
        Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true, block: $0)
    }

    var wantsAnimation: Bool { false }
    var animationIsVisible: Bool {
        window?.occlusionState.contains(.visible) == true && !isHiddenOrHasHiddenAncestor
    }

    func animationTick() { needsDisplay = true }

    func updateAnimation() {
        guard wantsAnimation && animationIsVisible else {
            animationTimer?.invalidate()
            animationTimer = nil
            return
        }
        guard animationTimer == nil else { return }
        animationTimer = makeAnimationTimer { [weak self] timer in
            // Keep the callback's non-Sendable Timer in its original isolation
            // region. Production scheduling and manual test delivery are on main.
            guard let self else {
                // The run loop owns repeating timers even after their view dies.
                timer.invalidate()
                return
            }
            MainActor.assumeIsolated {
                guard self.wantsAnimation && self.animationIsVisible else {
                    self.updateAnimation()
                    return
                }
                self.animationTick()
            }
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        let center = NotificationCenter.default
        center.removeObserver(self, name: NSWindow.didChangeOcclusionStateNotification, object: nil)
        if let window {
            center.addObserver(self, selector: #selector(visibilityChanged(_:)),
                               name: NSWindow.didChangeOcclusionStateNotification, object: window)
        }
        updateAnimation()
        needsDisplay = true
    }

    override func viewDidHide() {
        super.viewDidHide()
        updateAnimation()
    }

    override func viewDidUnhide() {
        super.viewDidUnhide()
        updateAnimation()
        needsDisplay = true
    }

    @objc private func visibilityChanged(_ notification: Notification) {
        updateAnimation()
        needsDisplay = true
    }
}
