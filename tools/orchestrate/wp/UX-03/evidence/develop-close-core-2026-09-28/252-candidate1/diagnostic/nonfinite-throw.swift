import Foundation
let value: [String: Any] = ["tone": ["exposure": Double.nan]]
FileHandle.standardError.write(Data("valid=\(JSONSerialization.isValidJSONObject(value))\n".utf8))
do { _ = try JSONSerialization.data(withJSONObject: value); print("encoded") } catch { print("threw: \(error)") }
