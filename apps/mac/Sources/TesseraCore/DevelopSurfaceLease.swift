import IOSurface

/// Prevents producer reuse while a mailbox, displayed frame, or Metal command owns it.
/// Acquired synchronously inside the engine callback, before the next producer runs.
public final class DevelopSurfaceLease: @unchecked Sendable, Equatable {
    public let surface: IOSurfaceRef
    public init?(id: UInt32) {
        guard id != 0, let surface = IOSurfaceLookup(id) else { return nil }
        self.surface = surface
        IOSurfaceIncrementUseCount(surface)
    }
    deinit { IOSurfaceDecrementUseCount(surface) }
    public static func == (lhs: DevelopSurfaceLease, rhs: DevelopSurfaceLease) -> Bool {
        IOSurfaceGetID(lhs.surface) == IOSurfaceGetID(rhs.surface)
    }
}
