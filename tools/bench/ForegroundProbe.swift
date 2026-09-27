import AppKit
// Read-only foreground identity. Does not create NSApplication, activate, or use Accessibility.
print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1)
