import AppKit

/// Native workspace geometry follows stable logical identity, rather than its
/// current window. This is native preference state, not portable session geometry.
@MainActor
final class WorkspaceSplitPreferences {
    private let defaults: UserDefaults
    private var namespace: UUID?
    private var identities: [UInt64: UUID] = [:]
    private var liveSplits: [UInt64: Set<String>] = [:]
    private var values: [String: [String: Double]] = [:]
    private var key: String { "workspaceSplitRatios." + (namespace?.uuidString.lowercased() ?? "unbound") }

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func reconcile(_ snapshot: DesktopSnapshot) throws {
        var next: [UInt64: UUID] = [:]
        for identity in snapshot.workspaceIdentities {
            guard next.updateValue(identity.uuid, forKey: identity.workspaceId) == nil else {
                throw BridgeFailure(message: "Duplicate workspace geometry identity")
            }
        }
        guard Set(next.keys) == Set(snapshot.model.workspaces.map(\.id)) else {
            throw BridgeFailure(message: "Workspace geometry identities differ from live membership")
        }
        if namespace != snapshot.model.identityNamespace {
            // One production profile changes namespace through explicit import.
            // Do not retain geometry for replaced logical identities indefinitely.
            if namespace != nil { defaults.removeObject(forKey: key) }
            namespace = snapshot.model.identityNamespace
            values = defaults.dictionary(forKey: key) as? [String: [String: Double]] ?? [:]
        }
        identities = next
        liveSplits = Dictionary(uniqueKeysWithValues: snapshot.model.workspaces.map { ($0.id, Set($0.root.splitKeys)) })
        var retained: [String: [String: Double]] = [:]
        for (workspace, uuid) in identities {
            let id = uuid.uuidString.lowercased()
            let splits = values[id, default: [:]].filter { split, fraction in
                liveSplits[workspace]?.contains(split) == true && fraction.isFinite && (0.2...0.8).contains(fraction)
            }
            if !splits.isEmpty { retained[id] = splits }
        }
        if values != retained { values = retained; persist() }
    }
    func fraction(workspace: UInt64, split: String) -> CGFloat {
        guard let uuid = identities[workspace] else { return 0.5 }
        return CGFloat(values[uuid.uuidString.lowercased()]?[split] ?? 0.5)
    }
    func remember(workspace: UInt64, split: String, fraction: CGFloat) {
        guard fraction.isFinite, (0.2...0.8).contains(fraction),
              liveSplits[workspace]?.contains(split) == true, let uuid = identities[workspace] else { return }
        let id = uuid.uuidString.lowercased()
        guard values[id]?[split] != Double(fraction) else { return }
        values[id, default: [:]][split] = Double(fraction)
        persist()
    }
    private func persist() {
        if values.isEmpty { defaults.removeObject(forKey: key) }
        else { defaults.set(values, forKey: key) }
    }
}
