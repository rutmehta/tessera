import AppKit

/// Keeps the main window inside the screen's visible frame (UX-16 item 36): the owner's window
/// sat under the Dock, with the agent field and the Try cards behind it. Applied when the window
/// is configured (first launch and a restored frame), on zoom (`windowWillUseStandardFrame`) and
/// when the window lands on another screen. Full-screen windows are left alone.
enum WindowFrameFitting {
    /// `frame` no larger than `visible`, moved inside it: the Dock edge pulls the window up, the
    /// menu bar edge pushes it down, and a frame wider or taller than the screen shrinks.
    static func fit(_ frame: CGRect, in visible: CGRect) -> CGRect {
        guard visible.width > 0, visible.height > 0 else { return frame }
        let width = min(frame.width, visible.width)
        let height = min(frame.height, visible.height)
        let x = min(max(frame.minX, visible.minX), visible.maxX - width)
        let y = min(max(frame.minY, visible.minY), visible.maxY - height)
        return CGRect(x: x, y: y, width: width, height: height)
    }

    /// Moves `window` inside its screen's visible frame when it is not already there.
    @MainActor
    static func apply(to window: NSWindow) {
        guard !window.styleMask.contains(.fullScreen), let screen = window.screen ?? NSScreen.main else { return }
        let fitted = fit(window.frame, in: screen.visibleFrame)
        if fitted != window.frame { window.setFrame(fitted, display: true) }
    }
}
