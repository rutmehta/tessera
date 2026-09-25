import AppKit
import Observation
import SwiftUI
import TesseraCore
import TesseraFFI

/// Soft proofing in Develop (S; docs/01 §2.26): the loupe simulates a printer/paper profile with
/// the chosen intent and paper-white simulation, and can paint out-of-gamut colours (⇧S). The
/// engine returns a presentation LUT (`DevelopSession.soft_proof`); the Metal presenter applies it
/// while sampling, so toggling never re-renders and never touches the recipe or exports.
@MainActor @Observable
final class SoftProof {
    static let shared = SoftProof()

    enum Intent: String, CaseIterable, Identifiable {
        case perceptual, relative
        var id: String { rawValue }
        var title: String { self == .perceptual ? "Perceptual" : "Relative" }
        var ffi: RenderingIntent { self == .perceptual ? .perceptual : .relativeColorimetric }
    }

    var enabled = false { didSet { if enabled != oldValue { refresh() } } }
    var profilePath: String? { didSet { if profilePath != oldValue { save(); refresh() } } }
    var intent: Intent = .perceptual { didSet { if intent != oldValue { save(); refresh() } } }
    var blackPointCompensation = true { didSet { if blackPointCompensation != oldValue { save(); refresh() } } }
    var simulatePaper = false { didSet { if simulatePaper != oldValue { save(); refresh() } } }
    var gamutWarning = false { didSet { if gamutWarning != oldValue { save(); onChange?() } } }
    /// Warning overlay colour (sRGB).
    var warningColor = Color(red: 1, green: 0, blue: 1) { didSet { onChange?() } }

    private(set) var profiles: [PrinterProfile] = []
    /// The LUT in use (nil: proofing off or not ready).
    private(set) var lut: SoftProofLut?
    private(set) var status = ""

    /// The loupe re-renders with the current LUT / warning colour.
    @ObservationIgnored var onChange: (() -> Void)?
    @ObservationIgnored weak var develop: DevelopController?
    @ObservationIgnored private var generation = 0
    /// Observation's generated accessors run `didSet` during init too; react only afterwards.
    @ObservationIgnored private var ready = false

    private init() {
        let d = UserDefaults.standard
        profilePath = d.string(forKey: "SoftProof.profile")
        intent = Intent(rawValue: d.string(forKey: "SoftProof.intent") ?? "") ?? .perceptual
        blackPointCompensation = d.object(forKey: "SoftProof.bpc") as? Bool ?? true
        simulatePaper = d.bool(forKey: "SoftProof.paper")
        gamutWarning = d.bool(forKey: "SoftProof.gamut")
        ready = true
    }

    private func save() {
        guard ready else { return }
        let d = UserDefaults.standard
        d.set(profilePath, forKey: "SoftProof.profile")
        d.set(intent.rawValue, forKey: "SoftProof.intent")
        d.set(blackPointCompensation, forKey: "SoftProof.bpc")
        d.set(simulatePaper, forKey: "SoftProof.paper")
        d.set(gamutWarning, forKey: "SoftProof.gamut")
    }

    func loadProfiles() {
        guard profiles.isEmpty else { return }
        profiles = printerProfiles()
        if profilePath == nil || !profiles.contains(where: { $0.path == profilePath }) {
            profilePath = profilePath.flatMap { p in (try? describePrinterProfile(path: p)).map { profiles.append($0); return p } }
                ?? profiles.first?.path
        }
    }

    func toggle() {
        loadProfiles()
        enabled.toggle()
    }

    /// Follows the session on screen (the LUT itself is per profile, not per image).
    func attach(_ controller: DevelopController?) {
        develop = controller
        refresh()
    }

    var warningRGB: SIMD4<Float> {
        let c = NSColor(warningColor).usingColorSpace(.sRGB) ?? .magenta
        let lin = { (v: CGFloat) -> Float in
            let v = Float(v)
            return v <= 0.04045 ? v / 12.92 : powf((v + 0.055) / 1.055, 2.4)
        }
        return SIMD4(lin(c.redComponent), lin(c.greenComponent), lin(c.blueComponent), gamutWarning ? 1 : 0)
    }

    private func refresh() {
        guard ready else { return }
        generation += 1
        let gen = generation
        guard enabled else {
            lut = nil
            status = ""
            onChange?()
            return
        }
        loadProfiles()
        guard let path = profilePath else {
            lut = nil
            status = "No printer profiles installed. Choose one with Other…"
            onChange?()
            return
        }
        guard let session = develop?.session else {
            lut = nil
            status = "Soft proofing shows on RAW photos in the loupe"
            onChange?()
            return
        }
        status = "Preparing proof…"
        let options = SoftProofOptions(profilePath: path, intent: intent.ffi,
                                       blackPointCompensation: blackPointCompensation, simulatePaper: simulatePaper)
        Task.detached(priority: .userInitiated) {
            let result = Result { try session.softProof(options: options) }
            await MainActor.run {
                guard gen == self.generation else { return }
                switch result {
                case .success(let lut):
                    self.lut = lut
                    self.status = lut.map { String(format: "Proofing %@ · %.0f%% of colours out of gamut", $0.profileName, $0.outOfGamut * 100) } ?? ""
                case .failure(let e):
                    self.lut = nil
                    self.status = e.localizedDescription
                }
                self.onChange?()
            }
        }
    }

    func chooseProfileFile() {
        let panel = NSOpenPanel()
        panel.title = "Soft Proof Profile"
        panel.message = "Choose an ICC output profile for a printer and paper"
        panel.allowedContentTypes = [.init(filenameExtension: "icc"), .init(filenameExtension: "icm")].compactMap { $0 }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let p = try describePrinterProfile(path: url.path)
            if !profiles.contains(where: { $0.path == p.path }) { profiles.append(p) }
            profilePath = p.path
        } catch { status = error.localizedDescription }
    }
}

/// Inspector panel: the toggle and its options.
struct SoftProofPanel: View {
    @Bindable var proof: SoftProof

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle("Soft proofing  (S)", isOn: Binding(get: { proof.enabled }, set: { _ in proof.toggle() }))
                .font(.system(size: 11))
                .accessibilityIdentifier("softproof-toggle")
            HStack {
                Picker("Profile", selection: Binding(get: { proof.profilePath ?? "" },
                                                     set: { proof.profilePath = $0.isEmpty ? nil : $0 })) {
                    if proof.profiles.isEmpty { Text("None").tag("") }
                    ForEach(proof.profiles, id: \.path) { p in Text(p.name).tag(p.path) }
                }
                Button("Other…") { proof.chooseProfileFile() }.controlSize(.small)
            }
            Picker("Intent", selection: $proof.intent) {
                ForEach(SoftProof.Intent.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            Toggle("Simulate paper and ink", isOn: $proof.simulatePaper)
            Toggle("Black point compensation", isOn: $proof.blackPointCompensation)
            HStack {
                Toggle("Gamut warning  (⇧S)", isOn: $proof.gamutWarning)
                Spacer()
                ColorPicker("", selection: $proof.warningColor, supportsOpacity: false).labelsHidden()
            }
            if !proof.status.isEmpty {
                Text(proof.status).font(.system(size: 10)).foregroundStyle(.secondary)
                    .accessibilityIdentifier("softproof-status")
            }
        }
        .font(.system(size: 11))
        .controlSize(.small)
        .onAppear { proof.loadProfiles() }
    }
}
