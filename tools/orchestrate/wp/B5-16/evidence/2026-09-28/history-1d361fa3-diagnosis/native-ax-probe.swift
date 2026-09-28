import AppKit
@MainActor final class Target: NSObject {
 var calls = 0
 @objc func clicked(_ sender: Any?) { calls += 1 }
}
NSApplication.shared.setActivationPolicy(.prohibited)
let target = Target()
let button = NSButton(title: "+", target: target, action: #selector(Target.clicked(_:)))
button.bezelStyle = .inline
button.setAccessibilityLabel("Increase History height")
let label = NSTextField(labelWithString: "168 pt")
label.setAccessibilityValue("168 points")
print("plain role=\(String(describing:button.accessibilityRole())) press=\(button.accessibilityPerformPress()) calls=\(target.calls) label=\(String(describing:label.accessibilityValue()))")
let window = NSWindow(contentRect: NSRect(x:0,y:0,width:200,height:80),styleMask:.titled,backing:.buffered,defer:false)
window.isReleasedWhenClosed = false
window.contentView?.addSubview(button)
window.contentView?.addSubview(label)
print("hosted role=\(String(describing:button.accessibilityRole())) press=\(button.accessibilityPerformPress()) calls=\(target.calls) label=\(String(describing:label.accessibilityValue()))")
window.contentView = nil
window.close()
