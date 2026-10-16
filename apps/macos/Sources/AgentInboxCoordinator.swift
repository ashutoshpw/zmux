import AppKit

@MainActor
final class AgentInboxDrafts {
    struct Key: Hashable { let origin: MainWindowIdentity; let parent: UUID }
    struct Draft { let body: String; let revision: UInt64 }
    private var drafts: [Key: Draft] = [:]
    private var order: [Key] = []
    private var revision: UInt64 = 0
    private var pinned: Set<Key> = []
    func value(_ key: Key) -> Draft { drafts[key] ?? Draft(body: "", revision: 0) }
    func set(_ key: Key, body: String) {
        guard body.utf8.count <= 4096 else { return }
        revision &+= 1
        drafts[key] = Draft(body: body, revision: revision)
        order.removeAll { $0 == key }; order.append(key)
        while order.count > 128, let victim = order.first(where: { !pinned.contains($0) }) {
            order.removeAll { $0 == victim }; drafts.removeValue(forKey: victim)
        }
    }
    func pin(_ key: Key) { pinned.insert(key) }
    func unpin(_ key: Key) { pinned.remove(key) }
    func clear(_ key: Key, ifRevision expected: UInt64) {
        defer { pinned.remove(key) }
        guard value(key).revision == expected else { return }
        drafts.removeValue(forKey: key); order.removeAll { $0 == key }
    }
}

/// One controller per exact native main-window identity, one shared bounded draft owner.
@MainActor
final class AgentInboxCoordinator {
    private let client: AgentMessageClient
    private let drafts = AgentInboxDrafts()
    private var controllers: [MainWindowIdentity: AgentInboxWindowController] = [:]
    private var stopped = false
    init(bridge: NativeBridge) { client = AgentMessageClient(bridge: bridge) }
    func show(from host: MainWindowHost) {
        guard !stopped, host.nativeReady, let origin = host.identity else { return }
        let controller: AgentInboxWindowController
        if let existing = controllers[origin] { controller = existing }
        else {
            controller = AgentInboxWindowController(origin: origin, client: client, drafts: drafts)
            controllers[origin] = controller
        }
        controller.show(relativeTo: host.window)
    }
    func update(_ snapshot: DesktopSnapshot) {
        let live = Set(snapshot.model.windows.map { MainWindowIdentity(namespace: snapshot.model.identityNamespace, windowId: $0.id) })
        for identity in Array(controllers.keys) where !live.contains(identity) {
            controllers.removeValue(forKey: identity)?.invalidateOrigin()
        }
    }
    func stop() {
        stopped = true
        for controller in controllers.values { controller.invalidateOrigin() }
        controllers.removeAll()
    }
}
