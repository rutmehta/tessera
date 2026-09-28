import Darwin
import Foundation

/// Same-directory staged publication. Exists checks are never overwrite admission.
/// Hooks are per-call source-test seams; production supplies none.
enum DocumentSaveDestinationCommit {
    struct Hooks: Sendable {
        var beforeCommit: (@Sendable (URL) throws -> Void)? = nil
        var afterCommit: (@Sendable (URL) throws -> Void)? = nil
        var exclusiveRename: (@Sendable (String, String) -> Int32)? = nil
        var unlinkStage: (@Sendable (String) -> Int32)? = nil
    }

    static func write(_ data: Data, to destination: URL, intent: DocSaveDestinationIntent,
                      hooks: Hooks = Hooks()) throws -> DocSaveAsResult {
        let parent = destination.deletingLastPathComponent()
        var template = Array(parent.appendingPathComponent(".tessera-save-XXXXXX").path.utf8CString)
        let descriptor = template.withUnsafeMutableBufferPointer { Darwin.mkstemp($0.baseAddress!) }
        guard descriptor >= 0 else { throw ioError("create staging file", errno) }
        let stagePath = template.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
        let stage = URL(fileURLWithPath: stagePath)
        var ownsStageName = true
        var identity = stat()
        guard Darwin.fstat(descriptor, &identity) == 0 else {
            let code = errno
            // Identity was not established; do not unlink an unverified path.
            Darwin.close(descriptor)
            diagnostic("Could not identify staging file; retained \(stagePath)", code)
            throw ioError("identify staging file", code)
        }
        func cleanupStage() {
            guard ownsStageName else { return }
            // Attempt at most once. In particular, never retry an old name after
            // successful rename/unlink where another writer may reuse that name.
            ownsStageName = false
            var current = stat()
            guard Darwin.lstat(stagePath, &current) == 0 else {
                if errno != ENOENT { diagnostic("Could not inspect staging file \(stagePath)", errno) }
                return
            }
            guard current.st_dev == identity.st_dev, current.st_ino == identity.st_ino else {
                diagnostic("Staging name no longer identifies our file; retained \(stagePath)", 0)
                return
            }
            let result = hooks.unlinkStage?(stagePath) ?? Darwin.unlink(stagePath)
            if result != 0 { diagnostic("Could not remove staging file \(stagePath)", errno) }
        }
        defer {
            cleanupStage()
            // Keep the descriptor through publication/cleanup to pin this inode.
            // Closing an already-published file cannot turn it into a failed save.
            if Darwin.close(descriptor) != 0 { diagnostic("Could not close staging descriptor", errno) }
        }
        try data.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return }
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(descriptor, base.advanced(by: offset), bytes.count - offset)
                if count < 0 {
                    if errno == EINTR { continue }
                    throw ioError("write staging file", errno)
                }
                guard count > 0 else { throw ioError("write staging file", EIO) }
                offset += count
            }
        }
        while Darwin.fsync(descriptor) != 0 {
            if errno == EINTR { continue }
            throw ioError("sync staging file", errno)
        }
        try hooks.beforeCommit?(stage)

        // A test may deliberately disturb the stage. Refuse to publish another
        // file at that name. This is not a hostile-directory identity-CAS claim.
        var current = stat()
        guard Darwin.lstat(stagePath, &current) == 0 else { throw ioError("inspect staging file", errno) }
        guard current.st_dev == identity.st_dev, current.st_ino == identity.st_ino else {
            ownsStageName = false
            throw DocumentError.io("Save staging file was replaced before publication")
        }
        switch intent {
        case .replaceConfirmed:
            guard Darwin.rename(stagePath, destination.path) == 0 else { throw ioError("replace destination", errno) }
            ownsStageName = false
        case .createIfAbsent:
            let result = hooks.exclusiveRename?(stagePath, destination.path)
                ?? Darwin.renamex_np(stagePath, destination.path, UInt32(RENAME_EXCL))
            if result == 0 {
                ownsStageName = false
            } else {
                let code = errno
                if code == EEXIST { return .destinationExists }
                guard code == ENOSYS || code == EINVAL else { throw ioError("create destination exclusively", code) }
                // Compatibility only for unsupported exclusive rename; never on
                // collision/permission failure, and never a replacing fallback.
                guard Darwin.link(stagePath, destination.path) == 0 else {
                    let linkCode = errno
                    if linkCode == EEXIST { return .destinationExists }
                    throw ioError("link destination exclusively", linkCode)
                }
                // Publication has succeeded. Cleanup failure is diagnostic only;
                // reporting failure here would misrepresent an already-created file.
                cleanupStage()
            }
        }
        do { try hooks.afterCommit?(stage) }
        catch { diagnostic("Post-publication hook failed: \(error)", 0) }
        return .saved
    }
    private static func ioError(_ operation: String, _ code: Int32) -> DocumentError {
        .io("Could not \(operation): \(String(cString: strerror(code)))")
    }
    private static func diagnostic(_ message: String, _ code: Int32) {
        NSLog("Document save cleanup: %@ (errno %d)", message, code)
    }
}
