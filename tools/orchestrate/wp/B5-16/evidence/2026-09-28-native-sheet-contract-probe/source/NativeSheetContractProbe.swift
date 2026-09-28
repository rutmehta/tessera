import AppKit
import Foundation
import Darwin

// SOURCE ONLY / UNRUN on Machine B. Standalone diagnostic; no Tessera dependency.
// One scenario per process. Watchdogs may fail a run, never satisfy the native join.
@MainActor
final class NativeSheetContractProbe: NSObject, NSApplicationDelegate {
    enum Scenario: String { case queuedCancel = "queued-cancel", parentClose = "parent-close", endThenClose = "end-then-parent-close", closeHandlerEnd = "close-handler-end" }
    let scenario: Scenario
    let orderOutInCompletion: Bool
    let invocation = UUID().uuidString
    let parentToken = UUID().uuidString
    let unrelatedToken = UUID().uuidString
    private var parent: NSWindow?
    private var owned: NSWindow?
    private var unrelated: NSWindow?
    private var observers: [NSObjectProtocol] = []
    private var watchdog: DispatchSourceTimer?
    private var sequence = 0
    private var phase = "setup"
    private var ownedCompletions = 0
    private var unrelatedCompletions = 0
    private var parentCloseObserved = false
    private var actionReturned = false
    private var ownedEndIssued = false
    private var completionInFlight = false
    private var finalized = false
    private var exitStatus: Int32 = 1

    init(scenario: Scenario, orderOutInCompletion: Bool) {
        self.scenario = scenario; self.orderOutInCompletion = orderOutInCompletion
    }
    private func identity(_ window: NSWindow?) -> String {
        window.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
    }
    private func member(_ window: NSWindow?) -> Bool {
        guard let parent, let window else { return false }
        return parent.attachedSheet === window || parent.sheets.contains { $0 === window }
    }
    private func emit(_ event: String, _ extra: [String: Any] = [:]) {
        sequence += 1
        var row: [String: Any] = [
            "sequence": sequence, "event": event, "phase": phase,
            "scenario": scenario.rawValue, "orderOutInCompletion": orderOutInCompletion,
            "invocation": invocation, "parentToken": parentToken,
            "uptime": ProcessInfo.processInfo.systemUptime,
            "parent": identity(parent), "owned": identity(owned), "unrelated": identity(unrelated),
            "attached": identity(parent?.attachedSheet),
            "sheets": parent?.sheets.map { identity($0) } ?? [],
            "ownedMember": member(owned), "unrelatedMember": member(unrelated),
            "ownedSheetParent": identity(owned?.sheetParent),
            "unrelatedSheetParent": identity(unrelated?.sheetParent),
            "ownedCompletions": ownedCompletions, "unrelatedCompletions": unrelatedCompletions,
            "parentCloseObserved": parentCloseObserved, "actionReturned": actionReturned
        ]
        for (key, value) in extra { row[key] = value }
        do {
            let data = try JSONSerialization.data(withJSONObject: row, options: [.sortedKeys])
            FileHandle.standardOutput.write(data)
            FileHandle.standardOutput.write(Data([10]))
        } catch {
            FileHandle.standardError.write(Data("Probe JSON failure: \(error)\n".utf8))
        }
    }
    private func window(_ title: String, width: CGFloat, height: CGFloat) -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: width, height: height),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = title
        window.isReleasedWhenClosed = false
        let label = NSTextField(labelWithString: title)
        label.frame = NSRect(x: 12, y: 12, width: width - 24, height: 26)
        window.contentView?.addSubview(label)
        return window
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        emit("launch", ["os": ProcessInfo.processInfo.operatingSystemVersionString,
                        "pid": ProcessInfo.processInfo.processIdentifier, "sourceStatus": "diagnostic"])
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now() + 5)
        timer.setEventHandler { [weak self] in
            MainActor.assumeIsolated {
                self?.finish(false, reason: "watchdog: native completion/membership contract not observed in 5 seconds")
            }
        }
        watchdog = timer; timer.resume()
        let parent = window("Disposable native sheet probe", width: 300, height: 120)
        let owned = window("Owned invocation", width: 220, height: 70)
        self.parent = parent; self.owned = owned
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.didEndSheetNotification,
            object: parent, queue: .main) { [weak self] note in
                let notificationParent = note.object.map { String(describing: ObjectIdentifier($0 as AnyObject)) } ?? "nil"
                let hasUserInfo = note.userInfo != nil
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.emit("parent.didEndSheet", ["notificationParent": notificationParent,
                                                     "hasUserInfo": hasUserInfo])
                    // Parent event contains no owned-sheet identity. Never increments completion.
                    self.evaluate()
                }
            })
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification,
            object: parent, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.parentCloseObserved = true
                    self.emit("parent.willClose")
                    if self.scenario == .closeHandlerEnd, self.phase != "cleanup",
                       !self.ownedEndIssued, self.ownedCompletions == 0,
                       let capturedParent = self.parent, let capturedSheet = self.owned,
                       self.member(capturedSheet) {
                        self.ownedEndIssued = true
                        self.emit("willClose.owned.end.call")
                        capturedParent.endSheet(capturedSheet, returnCode: .cancel)
                        self.emit("willClose.owned.end.return")
                    }
                    self.evaluate() // Close notification alone never proves detachment.
                }
            })
        parent.orderFront(nil)
        emit("parent.visible")
        guard parent.isVisible else { finish(false, reason: "setup: parent did not become visible"); return }
        if scenario == .queuedCancel {
            let other = window("Unrelated sentinel", width: 230, height: 75)
            unrelated = other
            let sentinel = unrelatedToken
            emit("unrelated.begin.call", ["unrelatedToken": sentinel])
            parent.beginSheet(other) { [weak self] response in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.unrelatedCompletions += 1
                    self.emit("unrelated.completion", ["response": response.rawValue, "unrelatedToken": sentinel])
                    self.evaluate()
                }
            }
            emit("unrelated.begin.return")
            guard parent.attachedSheet === other, member(other), unrelatedCompletions == 0 else {
                finish(false, reason: "setup: sentinel is not actually attached"); return
            }
        }
        let capturedInvocation = invocation
        emit("owned.begin.call")
        parent.beginSheet(owned) { [weak self] response in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.ownedCompletions += 1
                self.emit("owned.completion", ["response": response.rawValue, "callbackInvocation": capturedInvocation])
                // Raw mode measures native end alone. The explicit alternative
                // matches the adapter's real orderOut call; logs distinguish it
                // from native clearance, never treating it as a synthetic event.
                self.completionInFlight = true
                if self.orderOutInCompletion, self.phase != "cleanup", let capturedSheet = self.owned {
                    self.emit("owned.orderOut.call")
                    capturedSheet.orderOut(nil)
                    self.emit("owned.orderOut.return")
                }
                self.completionInFlight = false
                self.evaluate()
            }
        }
        emit("owned.begin.return")
        guard member(owned), ownedCompletions == 0 else {
            finish(false, reason: "setup: owned invocation was not admitted as a live sheet"); return
        }
        if scenario == .queuedCancel {
            guard let unrelated, parent.attachedSheet === unrelated,
                  let otherIndex = parent.sheets.firstIndex(where: { $0 === unrelated }),
                  let ownedIndex = parent.sheets.firstIndex(where: { $0 === owned }), otherIndex < ownedIndex else {
                finish(false, reason: "setup: owned sheet is not queued behind sentinel"); return
            }
            emit("queued-only.confirmed")
        } else if parent.attachedSheet !== owned {
            finish(false, reason: "setup: owned sheet was not attached before close experiment"); return
        }
        phase = "action"
        switch scenario {
        case .queuedCancel:
            emit("owned.end.call")
            ownedEndIssued = true
            parent.endSheet(owned, returnCode: .cancel) // ONLY our exact captured sheet.
            emit("owned.end.return")
        case .parentClose, .closeHandlerEnd:
            emit("parent.close.call")
            parent.close() // The explicit close-handler variant ends only our sheet in willClose.
            emit("parent.close.return")
        case .endThenClose:
            emit("owned.end.call")
            ownedEndIssued = true
            parent.endSheet(owned, returnCode: .cancel)
            emit("owned.end.return")
            emit("parent.close.call")
            parent.close()
            emit("parent.close.return")
        }
        actionReturned = true; phase = "observing"
        emit("action.returned")
        evaluate()
    }
    private func evaluate() {
        guard !finalized, !completionInFlight else { return }
        if ownedCompletions > 1 { finish(false, reason: "duplicate owned completion"); return }
        if scenario == .queuedCancel, unrelatedCompletions != 0 {
            finish(false, reason: "unrelated invocation completed before cleanup"); return
        }
        guard actionReturned, ownedCompletions == 1, !member(owned) else { return }
        if scenario == .queuedCancel {
            guard let parent, let unrelated, parent.attachedSheet === unrelated,
                  member(unrelated), unrelated.sheetParent === parent else {
                finish(false, reason: "unrelated captured sheet changed before cleanup"); return
            }
        } else if !parentCloseObserved { return }
        emit("owned.join.confirmed")
        finish(true, reason: "actual owned completion and exact parent membership clearance observed")
    }
    private func finish(_ passed: Bool, reason: String) {
        guard !finalized else { return }
        finalized = true; exitStatus = passed ? 0 : 1
        // Result is committed BEFORE cleanup, which can itself cause native callbacks.
        emit("result", ["passed": passed, "reason": reason,
                        "scope": "native invocation/membership only; not Tessera GUI or deallocation proof"])
        phase = "cleanup"
        watchdog?.cancel(); watchdog = nil
        observers.forEach { NotificationCenter.default.removeObserver($0) }; observers.removeAll()
        emit("observers.removed")
        if let parent {
            if let owned, member(owned), !ownedEndIssued, ownedCompletions == 0 {
                emit("cleanup.owned.end.call")
                ownedEndIssued = true
                parent.endSheet(owned, returnCode: .cancel)
            }
            if let unrelated, member(unrelated), unrelatedCompletions == 0 {
                emit("cleanup.unrelated.end.call")
                parent.endSheet(unrelated, returnCode: .cancel)
            }
        }
        owned?.orderOut(nil); unrelated?.orderOut(nil); parent?.orderOut(nil)
        owned?.close(); unrelated?.close(); parent?.close()
        owned = nil; unrelated = nil; parent = nil
        phase = "retired"
        emit("references.released", ["observerCount": observers.count,
                                      "claim": "probe-owned references only; native deferred release unmeasured"])
        // Process exit bounds diagnostic lifetime; never treat it as native completion.
        Darwin.exit(exitStatus)
    }
}

@main
struct ProbeMain {
    @MainActor static func main() {
        let arguments = CommandLine.arguments
        guard (arguments.count == 2 || (arguments.count == 3 && arguments[2] == "--order-out-in-completion")),
              let scenario = NativeSheetContractProbe.Scenario(rawValue: arguments[1]) else {
            FileHandle.standardError.write(Data("Usage: native-sheet-contract-probe queued-cancel|parent-close|end-then-parent-close|close-handler-end [--order-out-in-completion]\n".utf8))
            Darwin.exit(64)
        }
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let probe = NativeSheetContractProbe(scenario: scenario, orderOutInCompletion: arguments.count == 3)
        app.delegate = probe
        withExtendedLifetime(probe) { app.run() }
    }
}
