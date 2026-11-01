import Foundation

enum AgentAlertKind: String, Codable, CaseIterable, Hashable, Sendable {
    case needsUser = "needs_user"
    case completed

    var title: String {
        switch self {
        case .needsUser: "An agent needs you"
        case .completed: "An agent finished"
        }
    }
}

/// Built-in macOS alert tones. Provider configuration and MP3 files are not read.
enum AgentAlertSound: String, Codable, CaseIterable, Hashable, Sendable {
    case ping = "Ping"
    case glass = "Glass"
    case pop = "Pop"
    case purr = "Purr"
    case bottle = "Bottle"

    var title: String { rawValue }

    static func defaultSound(for kind: AgentAlertKind) -> Self {
        switch kind {
        case .needsUser: .ping
        case .completed: .glass
        }
    }
}

/// Zmux-owned alert choices. The standard UserDefaults domain persists them locally.
@MainActor
final class AgentAlertPreferences {
    private enum Key {
        static let playSounds = "zmux.agentAlerts.playSounds"
        static let needsUserSound = "zmux.agentAlerts.needsUserSound"
        static let completedSound = "zmux.agentAlerts.completedSound"
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    var playSounds: Bool {
        get { defaults.object(forKey: Key.playSounds) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.playSounds) }
    }

    var needsUserSound: AgentAlertSound {
        get { sound(for: .needsUser) }
        set { defaults.set(newValue.rawValue, forKey: Key.needsUserSound) }
    }

    var completedSound: AgentAlertSound {
        get { sound(for: .completed) }
        set { defaults.set(newValue.rawValue, forKey: Key.completedSound) }
    }

    func sound(for kind: AgentAlertKind) -> AgentAlertSound {
        let key: String
        switch kind {
        case .needsUser: key = Key.needsUserSound
        case .completed: key = Key.completedSound
        }
        guard let raw = defaults.string(forKey: key), let sound = AgentAlertSound(rawValue: raw) else {
            return .defaultSound(for: kind)
        }
        return sound
    }

}
