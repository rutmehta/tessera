import Foundation

// MARK: - Engine gaps (M2-48)

/// Develop controls whose recipe fields exist but which this engine build cannot render or export
/// yet. The panels show them disabled with the reason (see tools/orchestrate/wp/M2-48/REPORT.md).
/// Writing these fields today would make the photo fail to export, so they stay read-only until
/// the engine lifts the gap; flip the entry to nil then.
public enum DevelopEngineGaps {
    /// `denoise.method = neural`: the develop session renders it only after
    /// `DevelopSession.configureCfaDenoise` (a per-camera noise calibration the app cannot supply),
    /// and export has no denoiser at all, so an enabled AI Denoise fails every export.
    public static let aiDenoise: String? =
        "AI Denoise needs an engine denoiser this build does not install (no camera noise calibration, and export has no model backend). Classic noise reduction below still applies."
    /// `effects.lens_blur`: no depth map in the develop session or export (renders fail with
    /// "requires depth inference").
    public static let lensBlur: String? =
        "Lens Blur needs a depth map the engine does not compute here yet; applying it would make the photo fail to export."
    /// Depth plane for the focal-range histogram and Visualize Depth.
    public static let lensBlurDepth: String? = "No depth map from the engine yet"
    /// The recipe has no subject-aware field.
    public static let lensBlurSubject: String? = "Not in the recipe schema yet"
    /// No brush API for depth refinement.
    public static let lensBlurRefine: String? = "Focus / Blur refine brushes come later (no engine brush for depth)"
}

// MARK: - AI Denoise

/// Raw-domain AI Denoise (`denoise`): the neural CFA mode with the pinned model, plus its amount.
public enum AIDenoise {
    /// The pinned CFA U-Net (crates/ml-runtime/models.toml, task `cfa-denoise`).
    public static let modelID = "enhance/cfa-unet-fp32"
    public static let modelVersion = "a138c59a65846c10967839e85817231ec6ea92b318a57814cb153e8ac8bb311b"
    public static let methodPath = ["denoise", "method"]

    public static let amount = DevelopControl("Amount", ["denoise", "amount"], 0...100, default: 50,
                                              history: "AI Denoise Amount")

    /// Turns the neural mode on (with `amount`, when given) or back off. Off removes the model
    /// members so the merge leaves a clean `{"kind":"off"}` method.
    public static func patch(enabled: Bool, amount value: Double? = nil) -> [String: Any] {
        var denoise: [String: Any] = enabled
            ? ["method": ["kind": "neural", "model": ["id": modelID, "version": modelVersion], "joint_demosaic": false]]
            : ["method": ["kind": "off", "model": NSNull(), "joint_demosaic": NSNull()]]
        if let value { denoise["amount"] = amount.clamp(value) }
        return ["denoise": denoise]
    }

    public static func isEnabled(in settings: [String: Any]) -> Bool {
        (DevelopController.value(in: settings, at: methodPath + ["kind"]) as? String) == "neural"
    }

    public static func historyLabel(_ enabled: Bool) -> String { enabled ? "AI Denoise On" : "AI Denoise Off" }
}

// MARK: - Lens Blur

/// Bokeh shapes the engine's lens blur accepts (`circle`, `hexagon`, `octagon`).
public enum BokehShape: String, CaseIterable, Sendable, Identifiable {
    case circle, hexagon, octagon
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public static let path = ["effects", "lens_blur", "bokeh"]
}

/// The in-focus depth range (`effects.lens_blur.focus_range`, normalised near → far).
public struct FocalRange: Equatable, Sendable {
    /// Narrowest range the strip allows (keeps both handles grabbable).
    public static let minimumWidth = 0.02
    public private(set) var near: Double
    public private(set) var far: Double

    public init(near: Double, far: Double) {
        let a = min(max(near.isFinite ? near : 0, 0), 1), b = min(max(far.isFinite ? far : 0.1, 0), 1)
        (self.near, self.far) = (min(a, b), max(a, b))
        if self.far - self.near < Self.minimumWidth {
            self.far = min(self.near + Self.minimumWidth, 1)
            self.near = self.far - Self.minimumWidth
        }
    }

    /// The engine default `[0, 0.1]`.
    public static let standard = FocalRange(near: 0, far: 0.1)

    public init(settings: [String: Any]) {
        let v = (DevelopController.value(in: settings, at: LensBlurControls.focusRangePath) as? [NSNumber])?.map(\.doubleValue)
        self = v.map { $0.count == 2 ? FocalRange(near: $0[0], far: $0[1]) : .standard } ?? .standard
    }

    /// Moves one handle (the other stays), keeping the order and the minimum width.
    public mutating func setNear(_ v: Double) { self = FocalRange(near: min(v, far - Self.minimumWidth), far: far) }
    public mutating func setFar(_ v: Double) { self = FocalRange(near: near, far: max(v, near + Self.minimumWidth)) }
    /// Moves the whole range, clamped to `0…1`.
    public mutating func shift(by d: Double) {
        let w = far - near
        let n = min(max(near + d, 0), 1 - w)
        self = FocalRange(near: n, far: n + w)
    }

    public var patch: [String: Any] { DevelopController.patch(LensBlurControls.focusRangePath, [near, far]) }
    public var historyLabel: String { String(format: "Focal Range %.0f–%.0f", near * 100, far * 100) }
}

public enum LensBlurControls {
    public static let path = ["effects", "lens_blur"]
    public static let focusRangePath = ["effects", "lens_blur", "focus_range"]
    public static let amount = DevelopControl("Blur Amount", ["effects", "lens_blur", "amount"], 0...100, default: 50,
                                              history: "Lens Blur Amount")

    /// Apply on writes the engine defaults (amount 50, focus `[0, 0.1]`, circle); off removes the
    /// whole `lens_blur` member (the recipe's "not enabled").
    public static func applyPatch(_ on: Bool) -> [String: Any] {
        on ? ["effects": ["lens_blur": ["amount": amount.defaultValue, "focus_range": [0, 0.1], "bokeh": BokehShape.circle.rawValue]]]
            : ["effects": ["lens_blur": NSNull()]]
    }

    public static func bokehPatch(_ s: BokehShape) -> [String: Any] { DevelopController.patch(BokehShape.path, s.rawValue) }

    public static func isApplied(in settings: [String: Any]) -> Bool {
        DevelopController.value(in: settings, at: path) is [String: Any]
    }

    public static func bokeh(in settings: [String: Any]) -> BokehShape {
        (DevelopController.value(in: settings, at: BokehShape.path) as? String).flatMap(BokehShape.init(rawValue:)) ?? .circle
    }
}
