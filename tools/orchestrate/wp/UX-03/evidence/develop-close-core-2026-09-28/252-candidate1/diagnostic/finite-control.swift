import Foundation
let value: [String: Any] = ["tone": ["exposure": 1.25]]
print("valid=\(JSONSerialization.isValidJSONObject(value))")
do { print(String(decoding: try JSONSerialization.data(withJSONObject: value), as: UTF8.self)) } catch { print("threw: \(error)") }
