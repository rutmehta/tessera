import Foundation
let fm = FileManager.default
let dir = fm.temporaryDirectory.appendingPathComponent("save-url-diagnostic-\(UUID().uuidString)")
try fm.createDirectory(at: dir, withIntermediateDirectories: true)
defer { try? fm.removeItem(at: dir) }
let url = dir.appendingPathComponent("out.tessera-doc")
try Data("owned".utf8).write(to: url)
try Data("unowned".utf8).write(to: dir.appendingPathComponent(".stage"))
print("targetIdentity=\(try fm.attributesOfItem(atPath: url.path)[.systemFileNumber]!)")
print("temporary=\(fm.temporaryDirectory) target=\(url) targetPath=\(url.path)")
for entry in try fm.contentsOfDirectory(at: dir, includingPropertiesForKeys:nil) {
 print("entryIdentity=\(try fm.attributesOfItem(atPath: entry.path)[.systemFileNumber]!)")
 print("entry=\(entry) path=\(entry.path) equal=\(entry == url) pathEqual=\(entry.path == url.path) standardizedEqual=\(entry.standardizedFileURL == url.standardizedFileURL)")
}
