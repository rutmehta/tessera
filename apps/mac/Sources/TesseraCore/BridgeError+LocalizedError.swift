import Foundation
import TesseraFFI

/// UX-15 (item 24): an engine error shown anywhere in the app (a menu line, a toast, a sheet)
/// is its message, not `TesseraFFI.BridgeError.Failure(message: "…")`. The generated binding
/// omits its own `LocalizedError` conformance (`omit_localized_error_conformance` in
/// `crates/tessera-ffi/uniffi.toml`, whose `errorDescription` was `String(reflecting:)`), so
/// `localizedDescription` resolves here. Every surface shows `localizedDescription`; nothing
/// interpolates the error value itself.
extension BridgeError: LocalizedError {
    /// The engine's message for both cases.
    public var message: String {
        switch self {
        case .Failure(let message), .Busy(let message): message
        }
    }

    public var errorDescription: String? { message }
}
