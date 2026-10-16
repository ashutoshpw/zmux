import Foundation

struct MainWindowIdentity: Hashable, Sendable {
    let namespace: UUID
    let windowId: UInt64
}

/// A read projection for one native presenter. The coordinator retains the full
/// authoritative snapshot; this value never enters persistence or mutation APIs.
struct MainWindowProjection: Sendable {
    let identity: MainWindowIdentity
    let snapshot: DesktopSnapshot

    static func all(_ snapshot: DesktopSnapshot) throws -> [Self] {
        let model = snapshot.model
        guard model.windows.count <= 2 else { throw BridgeFailure(message: "Native main-window capacity exceeded.") }
        if model.windows.isEmpty {
            guard model.workspaces.isEmpty, model.focusedWindow == nil, model.focusedWorkspace == nil else { throw BridgeFailure(message: "The desktop has no canonical main-window owner.") }
            return []
        }
        let ids = model.windows.map(\.id)
        let catalogIds = model.workspaces.map(\.id)
        let membership = model.windows.flatMap(\.workspaceIds)
        guard ids.allSatisfy({ $0 > 0 }), Set(ids).count == ids.count,
              model.focusedWindow.map({ ids.contains($0) }) == true,
              Set(catalogIds).count == catalogIds.count,
              Set(membership).count == membership.count, Set(membership) == Set(catalogIds),
              model.windows.first(where: { $0.id == model.focusedWindow })?.selectedWorkspace == model.focusedWorkspace else { throw BridgeFailure(message: "The desktop has inconsistent canonical window ownership.") }
        let catalog = Dictionary(uniqueKeysWithValues: model.workspaces.map { ($0.id, $0) })
        return try model.windows.map { window in
            let members = Set(window.workspaceIds)
            guard members.isEmpty == (window.selectedWorkspace == nil),
                  window.selectedWorkspace.map({ members.contains($0) }) ?? members.isEmpty else { throw BridgeFailure(message: "The main window has inconsistent workspace selection.") }
            var projected = snapshot
            projected.model = Model(identityNamespace: model.identityNamespace, modelVersion: model.modelVersion,
                closedItems: model.closedItems, revision: model.revision, focusedWorkspace: window.selectedWorkspace,
                workspaces: window.workspaceIds.compactMap { catalog[$0] }, windows: [window], focusedWindow: window.id,
                workspaceGroups: model.workspaceGroups.filter { $0.windowId == window.id })
            return Self(identity: MainWindowIdentity(namespace: model.identityNamespace, windowId: window.id), snapshot: projected)
        }
    }
}
