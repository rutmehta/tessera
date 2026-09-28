import SwiftUI
@MainActor
func checkbox(_ on: Bool, _ change: @escaping @MainActor @Sendable (Bool) -> Void) -> some View {
    Toggle("Test", isOn: Binding(get: { on }, set: change))
}
