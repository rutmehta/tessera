import Foundation
import IOSurface

@main struct MailboxChecks {
    static func main() {
        let box = LatestFrameMailbox<Int>()
        precondition(box.offer(1, sequence: 1))
        precondition(!box.offer(3, sequence: 3))
        precondition(!box.offer(2, sequence: 2))
        precondition(box.take() == 3)
        precondition(!box.offer(1, sequence: 1))
        precondition(box.take() == nil)
        precondition(box.offer(4, sequence: 4))
        precondition(box.take() == 4)
        let props: [CFString: Any] = [kIOSurfaceWidth: 16, kIOSurfaceHeight: 16,
                                     kIOSurfaceBytesPerElement: 4, kIOSurfaceBytesPerRow: 64]
        let surface = IOSurfaceCreate(props as CFDictionary)!
        let before = IOSurfaceGetUseCount(surface)
        var lease: DevelopSurfaceLease? = DevelopSurfaceLease(id: IOSurfaceGetID(surface))
        precondition(lease != nil)
        precondition(IOSurfaceGetUseCount(surface) == before + 1)
        let leases = LatestFrameMailbox<DevelopSurfaceLease>()
        precondition(leases.offer(lease!))
        lease = nil
        precondition(IOSurfaceGetUseCount(surface) == before + 1)
        withExtendedLifetime(leases.take()) {}
        precondition(IOSurfaceGetUseCount(surface) == before)
        print("Mailbox replacement, old-generation rejection, and surface lifetime checks passed")
    }
}
