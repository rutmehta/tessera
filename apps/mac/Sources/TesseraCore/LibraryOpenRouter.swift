import Foundation
import Darwin

public enum LibraryAccessMode: Sendable, Equatable {
    case originalFolder, cachedSmartPreviews
    public var isReadOnly: Bool { self == .cachedSmartPreviews }
    public var label: String { isReadOnly ? "Cached Smart Preview Library · Catalog read-only" : "Original folder" }
    public var emptyMessage: String {
        "No cached Smart Preview declarations for this folder. Reconnect the original folder and build Smart Previews, then reopen it."
    }
    public func requireCatalogMutation() throws {
        guard !isReadOnly else { throw CullError.unavailable("Cached Smart Preview Library is read-only for culling, basket, people and metadata. Reconnect and reopen the original folder.") }
    }
}

/// One availability probe chooses a path. Never catch an online scan/decode error
/// and disguise it as offline success. All callers execute this on a worker.
public enum LibraryOpenRouter {
    public static func validateFolderPath(_ path: String) throws -> String {
        guard path.hasPrefix("/"), !path.split(separator: "/").contains("..") else {
            throw CullError.unavailable("Cached Library requires an absolute folder path without '..'")
        }
        return path // lexical identity; never canonicalize the absent original
    }

    public static func originalFolderAvailable(_ folder: URL) throws -> Bool {
        let path = try validateFolderPath(folder.path)
        var attributes = stat()
        let result = path.withCString { fstatat(AT_FDCWD, $0, &attributes, 0) }
        guard result == 0 else {
            let code = errno
            if code == ENOENT { return false }
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(code), userInfo: [NSFilePathErrorKey: path])
        }
        guard (attributes.st_mode & mode_t(S_IFMT)) == mode_t(S_IFDIR) else {
            throw CullError.unavailable("The original Library path is not a directory: \(path)")
        }
        return true
    }

    public static func open<Value>(folder: URL,
                                   availability: (URL) throws -> Bool = originalFolderAvailable,
                                   original: (URL) throws -> Value,
                                   cached: (URL) throws -> Value) throws -> Value {
        _ = try validateFolderPath(folder.path)
        if try availability(folder) { return try original(folder) }
        return try cached(folder)
    }
}
