import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// File ▸ Tethered Capture…: a non-modal panel docked at the top of the content area (under the
/// filter bar), so the grid or loupe stays live beside it. Header (camera, connect), session
/// (name, folder, naming with a live example), capture (⇧⌘T, interval) and the incoming strip.
struct TetherPanel: View {
    let model: AppModel
    @Bindable var tether: TetherController

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if let error = tether.error {
                StatusLine(text: error, kind: .error)
                    .lineLimit(2)
                    .padding(.horizontal, Theme.Space.gutter)
                    .padding(.bottom, Theme.Space.s)
                    .accessibilityIdentifier("tether-error")
            }
            if let notice = tether.notice {
                StatusLine(text: notice, kind: .warning)
                    .lineLimit(2)
                    .padding(.horizontal, Theme.Space.gutter)
                    .padding(.bottom, Theme.Space.s)
                    .accessibilityIdentifier("tether-notice")
            }
            if !tether.connected { devicesList }
            session
            if tether.connected { captureRow }
            if tether.connected || !tether.strip.frames.isEmpty { IncomingStripView(model: model, tether: tether) }
            Hairline()
        }
        .background(Theme.panel)
        .tint(Theme.accent)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("tether-panel")
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: Theme.Space.s) {
            Text("Tethered Capture").font(Theme.Fonts.labelSemibold).foregroundStyle(Theme.textPrimary)
            if tether.isFake {
                Chip(text: "Test camera", color: Theme.warning, style: .outlined)
                    .help("--fake-tether: frames come from \(tether.fakeSource?.path ?? "")")
            }
            if tether.connected, let d = tether.connectedDevice {
                HStack(spacing: Theme.Space.xs) {
                    Circle().fill(Theme.keep).frame(width: Theme.Space.s - Theme.Space.xxs, height: Theme.Space.s - Theme.Space.xxs)
                    Text("Connected: \(d.name)").foregroundStyle(Theme.textPrimary)
                }
                .font(Theme.Fonts.caption)
                .accessibilityIdentifier("tether-connected")
                DeviceReadouts(device: d)
            } else if tether.connecting {
                ProgressView().controlSize(.small)
                Text("Connecting…").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            }
            Spacer(minLength: Theme.Space.s)
            if tether.connected {
                Button("Disconnect") { tether.disconnect() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Close the camera session. Frames already downloaded stay.")
                    .accessibilityIdentifier("tether-disconnect")
            }
            IconButton(symbol: "xmark", help: "Hide the tether panel (the session keeps running)", size: Theme.Height.small) {
                tether.showPanel = false
            }
            .accessibilityIdentifier("tether-close")
        }
        .padding(.horizontal, Theme.Space.gutter)
        .frame(height: Theme.Height.sectionHeader)
    }

    // MARK: Devices

    private var devicesList: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.s) {
                Text("Camera").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .frame(width: Theme.Width.label, alignment: .leading)
                if tether.searching {
                    ProgressView().controlSize(.small)
                    Text("Looking for cameras…").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                } else if tether.devices.isEmpty {
                    StatusLine(text: tether.searched ? "No camera found. Connect it over USB, switch it on and set it to PC / tether mode."
                               : "Click Refresh to look for cameras.", kind: .warning)
                        .accessibilityIdentifier("tether-no-camera")
                } else if tether.devices.count > 1 {
                    StatusLine(text: "\(tether.devices.count) cameras are connected. Tether one camera at a time.", kind: .warning)
                }
                Spacer(minLength: Theme.Space.s)
                Button("Refresh") { tether.refreshDevices() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .disabled(tether.searching)
                    .accessibilityIdentifier("tether-refresh")
            }
            ForEach(Array(tether.devices.enumerated()), id: \.element.id) { i, device in
                HStack(spacing: Theme.Space.s) {
                    Spacer().frame(width: Theme.Width.label)
                    Image(systemName: "camera").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textSecondary)
                    Text(device.name).font(Theme.Fonts.label).foregroundStyle(Theme.textPrimary).lineLimit(1)
                    DeviceReadouts(device: device)
                    if !device.canCapture {
                        StatusLine(text: "No remote shutter: use the camera's button", kind: .warning)
                    }
                    Spacer(minLength: Theme.Space.s)
                    Button("Connect") { tether.connect() }
                        .buttonStyle(.theme(.primary, height: Theme.Height.small))
                        .disabled(!tether.canConnect)
                        .help("Start a session: frames download into \(tether.plannedFolder.path)")
                        .accessibilityIdentifier("tether-connect")
                }
                .frame(height: Theme.Height.regular)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("tether-device-\(i)")
            }
        }
        .padding(.horizontal, Theme.Space.gutter)
        .padding(.bottom, Theme.Space.s)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("tether-device-list")
    }

    // MARK: Session

    private var session: some View {
        HStack(alignment: .top, spacing: Theme.Space.m) {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                label("Session")
                FieldContainer(symbol: "folder.badge.plus") {
                    TextField("Session name", text: $tether.sessionName)
                        .textFieldStyle(.plain)
                        .font(Theme.Fonts.label)
                        .disabled(tether.connected)
                        .accessibilityIdentifier("tether-session-name")
                }
                .frame(width: 200)
                HStack(spacing: Theme.Space.xs) {
                    Text(tether.plannedFolder.path)
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        .lineLimit(1).truncationMode(.head)
                        .help(tether.plannedFolder.path)
                        .accessibilityIdentifier("tether-folder")
                    if !tether.connected {
                        Button("Choose…") { tether.chooseParentFolder() }
                            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                            .accessibilityIdentifier("tether-folder-choose")
                    }
                }
                .frame(maxWidth: 260, alignment: .leading)
            }
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                label("File names")
                HStack(spacing: Theme.Space.xs) {
                    FieldContainer(symbol: "textformat", invalid: namingProblem != nil) {
                        TextField("Naming template", text: $tether.template)
                            .textFieldStyle(.plain)
                            .font(Theme.Fonts.labelMono)
                            .disabled(tether.connected)
                            .accessibilityIdentifier("tether-naming")
                    }
                    .frame(width: 240)
                    Menu {
                        ForEach(TetherNaming.tokens, id: \.token) { t in
                            Button("\(t.title)  \(t.token)") { tether.template = insert(t.token) }
                        }
                        Divider()
                        Button("Reset to \(TetherNaming.defaultTemplate)") { tether.template = TetherNaming.defaultTemplate }
                    } label: {
                        Image(systemName: "plus").font(Theme.Fonts.iconSmall)
                    }
                    .menuStyle(IconMenuStyle())
                    .disabled(tether.connected)
                    .help("Insert a token")
                    .accessibilityIdentifier("tether-naming-token")
                }
                switch tether.namingExample {
                case .success(let name):
                    Text("\(TetherNaming.exampleOriginal) → \(name)")
                        .font(Theme.Fonts.captionMono).foregroundStyle(Theme.textSecondary)
                        .lineLimit(1).truncationMode(.middle)
                        .accessibilityIdentifier("tether-naming-example")
                case .failure(let problem):
                    StatusLine(text: problem.description, kind: .error)
                        .accessibilityIdentifier("tether-naming-example")
                }
            }
            if let album = tether.sessionAlbum {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    label("Albums")
                    HStack(spacing: Theme.Space.xs) {
                        Button(album) { model.setSource(.album(album)) }
                            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                            .help("Every frame of this session, in arrival order")
                            .accessibilityIdentifier("tether-album")
                        if tether.sessionSmartAlbum != nil {
                            Button(TetherController.smartAlbumName) { tether.showSessionSmartAlbum() }
                                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                                .help("Smart album scoped to this session's group: its frames that are not rejected")
                                .accessibilityIdentifier("tether-smart-album")
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Space.gutter)
        .padding(.bottom, Theme.Space.s)
    }

    private var namingProblem: TetherNaming.Problem? {
        if case .failure(let p) = tether.namingExample { return p }
        return nil
    }

    /// Tokens go before the extension so the name keeps its format.
    private func insert(_ token: String) -> String {
        let t = tether.template
        if token != "{ext}", let r = t.range(of: ".{ext}", options: .backwards) {
            return t.replacingCharacters(in: r, with: "_\(token).{ext}")
        }
        return t + token
    }

    private func label(_ text: String) -> some View {
        Text(text).font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
    }

    // MARK: Capture

    private var captureRow: some View {
        HStack(spacing: Theme.Space.s) {
            Button { tether.capture() } label: {
                Label("Capture", systemImage: "camera.shutter.button")
            }
            .buttonStyle(.theme(.primary, height: Theme.Height.regular))
            .disabled(!(tether.connectedDevice?.canCapture ?? true))
            .help("Fire the shutter (⇧⌘T). The physical shutter works too.")
            .accessibilityIdentifier("tether-capture")
            Text("⇧⌘T").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
            Hairline(vertical: true).frame(height: Theme.Space.m)
            Text("Every").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            MenuPicker(selection: $tether.intervalSeconds,
                       options: TetherController.intervalOptions.map { ($0, $0 < 60 ? "\(Int($0)) s" : "\(Int($0 / 60)) min") })
                .disabled(tether.interval != nil)
                .accessibilityIdentifier("tether-interval-seconds")
            MenuPicker(selection: $tether.intervalCount,
                       options: TetherController.countOptions.map { ($0, $0 == 0 ? "until stopped" : "\($0) frames") })
                .disabled(tether.interval != nil)
                .accessibilityIdentifier("tether-interval-count")
            if let plan = tether.interval {
                Button("Stop Interval") { tether.stopInterval() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("tether-interval-toggle")
                Text(plan.remaining.map { "\($0) left" } ?? "\(plan.taken) taken")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("tether-interval-status")
            } else {
                Button("Start Interval") { tether.startInterval() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .disabled(!(tether.connectedDevice?.canCapture ?? true))
                    .help("Shoot now, then every interval until the count is reached")
                    .accessibilityIdentifier("tether-interval-toggle")
            }
            Hairline(vertical: true).frame(height: Theme.Space.m)
            Toggle("Show newest in loupe", isOn: $tether.autoAdvance)
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textSecondary)
                .help("Auto-advance: each new frame opens in the loupe, ready for X / P / 1–3")
                .accessibilityIdentifier("tether-auto-advance")
            Spacer(minLength: Theme.Space.s)
            Text(tether.strip.summary)
                .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                .accessibilityIdentifier("tether-summary")
        }
        .padding(.horizontal, Theme.Space.gutter)
        .frame(height: Theme.Height.sectionHeader)
    }
}

/// Battery and remaining frames, when the camera reports them.
private struct DeviceReadouts: View {
    let device: TetherDevice
    var body: some View {
        HStack(spacing: Theme.Space.s) {
            if let b = device.batteryPercent {
                Label("\(b) %", systemImage: b <= 15 ? "battery.0percent" : b <= 50 ? "battery.50percent" : "battery.100percent")
                    .foregroundStyle(b <= 15 ? Theme.warning : Theme.textSecondary)
                    .help("Camera battery")
            }
            if let s = device.shotsRemaining {
                Label("\(s) left", systemImage: "sdcard")
                    .foregroundStyle(Theme.textSecondary)
                    .help("Frames left on the card")
            }
        }
        .font(Theme.Fonts.captionNumeric)
        .labelStyle(.titleAndIcon)
        .accessibilityIdentifier("tether-device-readouts")
    }
}

// MARK: - Incoming strip

/// Latest frames, newest first, each with its focus and eyes badges and the decision once made.
/// Click focuses a frame for the usual keys; double-click opens it in the loupe.
struct IncomingStripView: View {
    let model: AppModel
    let tether: TetherController
    private static let tileHeight = Theme.Height.filmstrip - Theme.Space.l
    private static let tileWidth = (Theme.Height.filmstrip - Theme.Space.l) * 3 / 2

    var body: some View {
        // Decisions live in the cull controller (unobserved); the counts are the observed proxy.
        let counts = model.counts
        let focusedID = model.focusedItem?.id
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: Theme.Space.s) {
                VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                    Text("Incoming").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                    StripLegend()
                }
                .frame(width: Theme.Width.labelWide, alignment: .leading)
                ScrollView(.horizontal) {
                    HStack(spacing: Theme.Space.s) {
                        ForEach(0..<tether.strip.pending, id: \.self) { _ in pendingTile }
                        ForEach(tether.strip.frames) { frame in
                            let id = tether.item(for: frame)
                            IncomingTile(frame: frame, image: tether.thumbnails[frame.sequence],
                                         state: id.map { model.state(id: $0) }, focused: id != nil && id == focusedID,
                                         width: Self.tileWidth, height: Self.tileHeight)
                                .onTapGesture(count: 2) { tether.reveal(frame, loupe: true) }
                                .onTapGesture { tether.reveal(frame, loupe: false) }
                        }
                        if tether.strip.frames.isEmpty && tether.strip.pending == 0 {
                            Hint(tether.isFake ? "Press Capture (⇧⌘T); the test camera also fires on its own timer."
                                 : "Press Capture (⇧⌘T) or the camera's shutter. Frames appear here once downloaded and scored.")
                        }
                    }
                }
                .scrollIndicators(.never)
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.filmstrip)
        }
        .accessibilityElement(children: .contain)
        .accessibilityValue("\(tether.strip.summary); \(counts.keep) kept, \(counts.reject) rejected")
        .accessibilityIdentifier("tether-incoming")
    }

    private var pendingTile: some View {
        VStack(spacing: Theme.Space.xs) {
            ProgressView().controlSize(.small)
            Text("Downloading").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
        }
        .frame(width: Self.tileWidth, height: Self.tileHeight)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Theme.well))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .help("Capture requested: downloading, indexing and scoring")
        .accessibilityIdentifier("tether-pending")
    }
}

private struct StripLegend: View {
    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            ForEach([(Theme.keep, "Sharp"), (Theme.warning, "Soft"), (Theme.reject, "Missed")], id: \.1) { color, title in
                Circle().fill(color).frame(width: Theme.Space.s - Theme.Space.xxs, height: Theme.Space.s - Theme.Space.xxs)
                    .help(title)
            }
            Image(systemName: "eye").font(Theme.Fonts.iconSmall).help("Eyes (faces only)")
        }
        .foregroundStyle(Theme.textTertiary)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Focus dot: green sharp, yellow soft, red missed. Eye: eyes open, warning, or closed.")
    }
}

private struct IncomingTile: View {
    let frame: IncomingFrame
    let image: CGImage?
    let state: CullState?
    let focused: Bool
    let width: CGFloat
    let height: CGFloat

    var body: some View {
        ZStack(alignment: .bottom) {
            if let image {
                Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fill)
                    .frame(width: width, height: height)
                    .clipped()
                    .opacity(state?.decision == .reject ? Double(Theme.Opacity.rejectedImage) : 1)
            } else {
                Theme.well
                Image(systemName: frame.failed ? "exclamationmark.triangle.fill" : "photo")
                    .font(Theme.Fonts.icon)
                    .foregroundStyle(frame.failed ? Theme.reject : Theme.textTertiary)
                    .frame(maxHeight: .infinity)
            }
            badges
        }
        .frame(width: width, height: height)
        .overlay(alignment: .topLeading) {
            if let badge = state?.badgeText, let state {
                Chip(text: badge, color: Color(nsColor: state.decision.color), style: .filled, height: Theme.Height.chip)
                    .padding(Theme.Space.xxs)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
            .strokeBorder(focused ? Theme.accent : Theme.hairlineStrong, lineWidth: focused ? Theme.Space.xxs : Theme.Space.hairline))
        .contentShape(Rectangle())
        .help(help)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(help)
        .accessibilityIdentifier("tether-frame-\(frame.sequence)")
    }

    private var badges: some View {
        HStack(spacing: Theme.Space.xs) {
            Text(String(format: "%04llu", frame.sequence))
                .font(Theme.Fonts.captionNumeric)
                .foregroundStyle(Theme.textSecondary)
            Spacer(minLength: 0)
            if !frame.failed {
                if frame.focus != nil {
                    Circle().fill(frame.focusLevel.color)
                        .frame(width: Theme.Space.s, height: Theme.Space.s)
                        .overlay(Circle().strokeBorder(Theme.shadow, lineWidth: Theme.Space.hairline))
                }
                if frame.hasFaces {
                    Image(systemName: frame.eyesLevel.eyesSymbol)
                        .font(Theme.Fonts.iconSmall)
                        .foregroundStyle(frame.eyesLevel.color)
                }
            }
        }
        .padding(.horizontal, Theme.Space.xs)
        .frame(height: Theme.Height.chip)
        .background(Theme.hud)
    }

    private var help: String {
        var parts = ["\(frame.name) (#\(frame.sequence))"]
        if let error = frame.error { parts.append("failed: \(error)"); return parts.joined(separator: " · ") }
        if let f = frame.focus {
            parts.append(String(format: "%@ %.2f · %@", frame.focusIsFace ? "Face focus" : "Sharpness", f, frame.focusLevel.focusWord))
        } else {
            parts.append("not scored yet")
        }
        switch frame.faces {
        case .some(0): parts.append("no faces")
        case .some(let n): parts.append("\(n) face\(n == 1 ? "" : "s") · \(frame.eyesLevel.eyesWord)")
        case .none: parts.append("faces not analysed" + (frame.faceWarning.map { ": \($0)" } ?? ""))
        }
        if let state, state.decision != .undecided { parts.append(state.decision.label) }
        return parts.joined(separator: " · ")
    }
}
