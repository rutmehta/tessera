import Foundation
import TesseraFFI

/// UI-safe engine calls (`Engine.roots` and `agentProvenance`; `CullSession.images`, `groups`,
/// `people`, `faceStrip`, `assistStatus` and `review`) give up with `BridgeError.Busy` after about
/// 20 ms instead of waiting out a worker that holds the catalog or the queue, so a call on the main
/// actor never freezes the app (ST-1). A main-actor caller keeps what it has and retries later; off
/// the main actor, waiting is what we want, which `waiting` does.
public enum EngineBusy {
    public static func isBusy(_ error: Error) -> Bool {
        if case BridgeError.Busy = error { return true }
        return false
    }

    /// Retries `body` while the engine reports busy, for up to `limit` seconds. Blocking: call off
    /// the main actor.
    public static func waiting<T>(limit: TimeInterval = 10, _ body: () throws -> T) throws -> T {
        let deadline = Date().addingTimeInterval(limit)
        while true {
            do {
                return try body()
            } catch where isBusy(error) && Date() < deadline {
                Thread.sleep(forTimeInterval: 0.02)
            }
        }
    }
}
