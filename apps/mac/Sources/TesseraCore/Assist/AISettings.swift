import Foundation
import Security
import TesseraFFI

/// Where API keys live. The app uses the Keychain; tests use `InMemorySecretStore`.
/// Implementations never log or print values.
public protocol SecretStore: AnyObject, Sendable {
    func read(account: String) throws -> String?
    /// Nil or empty deletes the item.
    func write(_ value: String?, account: String) throws
}

public enum SecretStoreError: LocalizedError, Equatable {
    case keychain(OSStatus)
    public var errorDescription: String? {
        switch self {
        case .keychain(let status):
            let text = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
            return "Keychain: \(text)"
        }
    }
}

/// Generic-password items in the login Keychain, one per provider account.
public final class KeychainSecretStore: SecretStore, @unchecked Sendable {
    public let service: String
    public init(service: String = "dev.tessera.app.ai") { self.service = service }

    private func query(_ account: String) -> [CFString: Any] {
        [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: account]
    }

    public func read(account: String) throws -> String? {
        var q = query(account)
        q[kSecReturnData] = true
        q[kSecMatchLimit] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = out as? Data else { throw SecretStoreError.keychain(status) }
        return String(data: data, encoding: .utf8)
    }

    public func write(_ value: String?, account: String) throws {
        let q = query(account)
        guard let value, !value.isEmpty else {
            let status = SecItemDelete(q as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else { throw SecretStoreError.keychain(status) }
            return
        }
        let data = Data(value.utf8)
        let update = SecItemUpdate(q as CFDictionary, [kSecValueData: data] as CFDictionary)
        if update == errSecItemNotFound {
            var add = q
            add[kSecValueData] = data
            add[kSecAttrAccessible] = kSecAttrAccessibleWhenUnlocked
            add[kSecAttrLabel] = "Tessera AI (\(account))"
            let status = SecItemAdd(add as CFDictionary, nil)
            guard status == errSecSuccess else { throw SecretStoreError.keychain(status) }
        } else if update != errSecSuccess {
            throw SecretStoreError.keychain(update)
        }
    }
}

/// A mock for tests and previews.
public final class InMemorySecretStore: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: String] = [:]
    public private(set) var writes = 0
    public init(_ values: [String: String] = [:]) { self.values = values }
    public func read(account: String) throws -> String? { lock.withLock { values[account] } }
    public func write(_ value: String?, account: String) throws {
        lock.withLock {
            writes += 1
            if let value, !value.isEmpty { values[account] = value } else { values.removeValue(forKey: account) }
        }
    }
}

/// Who plans the base edit (Settings ▸ AI and the Auto Edit sheet).
public enum AIProviderKind: String, Codable, CaseIterable, Sendable, Identifiable {
    case styleProfile, anthropic, openAI, ollama
    /// Hidden test aid (`--fake-planner`): the engine's FakePlanner with a deterministic script.
    case scripted

    public static var visible: [AIProviderKind] { [.styleProfile, .anthropic, .openAI, .ollama] }
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .styleProfile: "Style profile only"
        case .anthropic: "Anthropic"
        case .openAI: "OpenAI"
        case .ollama: "Ollama (local)"
        case .scripted: "Scripted test planner"
        }
    }
    /// Keychain account for providers that need a key.
    public var keyAccount: String? {
        switch self {
        case .anthropic: "anthropic-api-key"
        case .openAI: "openai-api-key"
        default: nil
        }
    }
    public var needsNetwork: Bool { self == .anthropic || self == .openAI }
}

/// Settings ▸ AI and the Auto Edit sheet's remembered choices. Stored as JSON under the app
/// directory (`ai-preferences.json`); keys are never part of it.
public struct AIPreferences: Codable, Equatable, Sendable {
    public var provider: AIProviderKind = .styleProfile
    public var anthropicModel = "claude-fable-5-1"
    public var openAIModel = "gpt-4.1"
    public var ollamaHost = "http://localhost:11434"
    public var ollamaModel = "qwen2.5:7b"
    public var ollamaVision = false
    // Guardrails (docs/10 §2 "Safety/scope").
    public var allowMasks = true
    public var allowCrop = false
    public var allowSkinRetouch = false
    public var visualCritic = false
    public var maxIterations = 3
    public var timeBudgetSeconds = 120
    // Consistency across a batch.
    public var sceneConsistency = true
    public var personConsistency = true
    // Assisted culling (automated thresholds).
    public var assistAutomated = true
    public var rejectBelow = 0.25
    public var keepAbove = 0.75

    public init() {}

    public init(from decoder: Decoder) throws {
        // Missing members (older files) keep their defaults.
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = AIPreferences()
        provider = (try? c.decode(AIProviderKind.self, forKey: .provider)) ?? d.provider
        anthropicModel = (try? c.decode(String.self, forKey: .anthropicModel)) ?? d.anthropicModel
        openAIModel = (try? c.decode(String.self, forKey: .openAIModel)) ?? d.openAIModel
        ollamaHost = (try? c.decode(String.self, forKey: .ollamaHost)) ?? d.ollamaHost
        ollamaModel = (try? c.decode(String.self, forKey: .ollamaModel)) ?? d.ollamaModel
        ollamaVision = (try? c.decode(Bool.self, forKey: .ollamaVision)) ?? d.ollamaVision
        allowMasks = (try? c.decode(Bool.self, forKey: .allowMasks)) ?? d.allowMasks
        allowCrop = (try? c.decode(Bool.self, forKey: .allowCrop)) ?? d.allowCrop
        allowSkinRetouch = (try? c.decode(Bool.self, forKey: .allowSkinRetouch)) ?? d.allowSkinRetouch
        visualCritic = (try? c.decode(Bool.self, forKey: .visualCritic)) ?? d.visualCritic
        maxIterations = min(max((try? c.decode(Int.self, forKey: .maxIterations)) ?? d.maxIterations, 1), 20)
        timeBudgetSeconds = max((try? c.decode(Int.self, forKey: .timeBudgetSeconds)) ?? d.timeBudgetSeconds, 10)
        sceneConsistency = (try? c.decode(Bool.self, forKey: .sceneConsistency)) ?? d.sceneConsistency
        personConsistency = (try? c.decode(Bool.self, forKey: .personConsistency)) ?? d.personConsistency
        assistAutomated = (try? c.decode(Bool.self, forKey: .assistAutomated)) ?? d.assistAutomated
        rejectBelow = (try? c.decode(Double.self, forKey: .rejectBelow)) ?? d.rejectBelow
        keepAbove = (try? c.decode(Double.self, forKey: .keepAbove)) ?? d.keepAbove
    }

    public var guardrails: AgentGuardrails {
        AgentGuardrails(allowMasks: allowMasks, allowCrop: allowCrop, allowSkinRetouch: allowSkinRetouch,
                        visualCritic: visualCritic, maxIterations: UInt32(min(max(maxIterations, 1), 20)),
                        timeBudgetSeconds: UInt32(max(timeBudgetSeconds, 10)))
    }

    public var assistMode: AssistMode {
        assistAutomated ? .automated(rejectBelow: rejectBelow, keepAbove: keepAbove) : .assisted
    }

    public func model(for provider: AIProviderKind) -> String {
        switch provider {
        case .anthropic: anthropicModel
        case .openAI: openAIModel
        case .ollama: ollamaModel
        case .styleProfile, .scripted: ""
        }
    }
}

public enum AISettingsError: LocalizedError, Equatable {
    case missingKey(AIProviderKind)
    public var errorDescription: String? {
        switch self {
        case .missingKey(let p): "Add an \(p.title) API key in Settings ▸ AI"
        }
    }
}

/// Preferences on disk plus keys in a `SecretStore`.
public final class AISettingsStore: @unchecked Sendable {
    public let file: URL
    public let secrets: SecretStore

    public init(directory: URL, secrets: SecretStore = KeychainSecretStore()) {
        file = directory.appendingPathComponent("ai-preferences.json")
        self.secrets = secrets
    }

    public func load() -> AIPreferences {
        guard let data = try? Data(contentsOf: file) else { return AIPreferences() }
        return (try? JSONDecoder().decode(AIPreferences.self, from: data)) ?? AIPreferences()
    }

    public func save(_ preferences: AIPreferences) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(preferences).write(to: file, options: .atomic)
    }

    public func apiKey(for provider: AIProviderKind) -> String? {
        guard let account = provider.keyAccount else { return nil }
        return (try? secrets.read(account: account)).flatMap { $0?.isEmpty == false ? $0 : nil }
    }

    public func hasKey(for provider: AIProviderKind) -> Bool { apiKey(for: provider) != nil }

    /// Stores (or with nil / whitespace, removes) a key. Surrounding whitespace is trimmed.
    public func setAPIKey(_ key: String?, for provider: AIProviderKind) throws {
        guard let account = provider.keyAccount else { return }
        let trimmed = key?.trimmingCharacters(in: .whitespacesAndNewlines)
        try secrets.write(trimmed?.isEmpty == false ? trimmed : nil, account: account)
    }

    /// "sk-a…9f2c": enough to recognise a key, never the key.
    public static func masked(_ key: String) -> String {
        guard key.count > 8 else { return String(repeating: "•", count: key.count) }
        return String(key.prefix(4)) + "…" + String(key.suffix(4))
    }

    /// The engine's provider for a run, reading the key at the last moment.
    public func provider(_ kind: AIProviderKind, _ p: AIPreferences) throws -> AgentProvider {
        switch kind {
        case .styleProfile: return .styleProfile
        case .scripted: return .scripted
        case .ollama: return .ollama(host: p.ollamaHost, model: p.ollamaModel, vision: p.ollamaVision)
        case .anthropic, .openAI:
            guard let key = apiKey(for: kind) else { throw AISettingsError.missingKey(kind) }
            return kind == .anthropic ? .anthropic(apiKey: key, model: p.anthropicModel)
                : .openAi(apiKey: key, model: p.openAIModel)
        }
    }
}
