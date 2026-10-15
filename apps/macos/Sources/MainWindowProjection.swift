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

    static func singleton(_ snapshot: DesktopSnapshot) throws -> Self? {
        let model = snapshot.model
        guard model.windows.count <= 1 else {
            throw BridgeFailure(message: "Multiple native main windows are not available.")
        }
        guard let window = model.windows.first else {
            // The Rust owner's never-started snapshot alone may have no window.
            guard model.workspaces.isEmpty, model.focusedWindow == nil, model.focusedWorkspace == nil else {
                throw BridgeFailure(message: "The desktop has no canonical main-window owner.")
            }
            return nil
        }
        let catalogIds = model.workspaces.map(\.id)
        let membership = Set(window.workspaceIds)
        guard window.id > 0, model.focusedWindow == window.id,
              Set(catalogIds).count == catalogIds.count,
              membership.count == window.workspaceIds.count,
              membership == Set(catalogIds),
              window.selectedWorkspace.map({ membership.contains($0) }) ?? membership.isEmpty,
              membership.isEmpty == (window.selectedWorkspace == nil),
              model.focusedWorkspace == window.selectedWorkspace else {
            throw BridgeFailure(message: "The main window has inconsistent workspace ownership or selection.")
        }
        let catalog = Dictionary(uniqueKeysWithValues: model.workspaces.map { ($0.id, $0) })
        var projected = snapshot
        projected.model = Model(
            identityNamespace: model.identityNamespace, modelVersion: model.modelVersion,
            closedItems: model.closedItems, revision: model.revision,
            focusedWorkspace: window.selectedWorkspace,
            workspaces: window.workspaceIds.compactMap { catalog[$0] },
            windows: [window], focusedWindow: window.id
        )
        return Self(identity: MainWindowIdentity(namespace: model.identityNamespace, windowId: window.id), snapshot: projected)
    }
}
