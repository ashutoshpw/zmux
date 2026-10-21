import AppKit
import Foundation

/// Exercises SSH reconnect through the packaged tab control and a private loopback sshd.
@MainActor
enum SSHReconnectSmoke {
    private struct Configuration: Decodable, Sendable {
        let clientConfig: String
        let identity: String
    }

    static func validate(
        configuration: URL,
        host: MainWindowHost,
        snapshot: @MainActor () -> DesktopSnapshot?,
        terminal: @MainActor (UInt64) -> TerminalView?
    ) async throws {
        let config = try await Task.detached {
            try JSONDecoder().decode(Configuration.self, from: Data(contentsOf: configuration))
        }.value
        guard let originalWorkspace = snapshot()?.model.focusedWorkspace else {
            throw BridgeFailure(message: "SSH reconnect smoke has no initial workspace")
        }

        var createdWorkspaces: [UInt64] = []
        do {
            _ = try await host.nativeCLI(["select-workspace", String(originalWorkspace)], externalCaller: true)
            try await waitUntil {
                snapshot()?.model.focusedWorkspace == originalWorkspace
                    && Self.tabs(in: host.window.contentView).contains {
                        guard $0.surface.kind == "terminal", $0.surface.remote == nil,
                              let session = $0.surface.sessionId,
                              snapshot()?.frames.first(where: { $0.sessionId == session })?.reconnectable == false,
                              let button = Self.reconnectButton(in: $0) else { return false }
                        return button.isHiddenOrHasHiddenAncestor && !button.isEnabled
                    }
            }

            _ = try await host.nativeCLI([
                "ssh", "fixture", "--ssh-config", config.clientConfig, "--identity", config.identity,
                "--name", "Native reconnect live"
            ], timeout: 15_000, externalCaller: true)
            let liveWorkspace = try await waitForWorkspace(named: "Native reconnect live", snapshot: snapshot)
            createdWorkspaces.append(liveWorkspace.id)
            _ = try await host.nativeCLI(["select-workspace", String(liveWorkspace.id)], externalCaller: true)
            try await waitUntil {
                guard let workspace = snapshot()?.model.workspaces.first(where: { $0.id == liveWorkspace.id }),
                      let pane = workspace.root.panes.first, let session = pane.sessionId,
                      let frame = snapshot()?.frames.first(where: { $0.sessionId == session }),
                      frame.snapshot?.state == "running", !frame.reconnectable,
                      let tab = Self.tab(surface: pane.selectedSurface, in: host.window.contentView),
                      let button = Self.reconnectButton(in: tab) else { return false }
                return button.isHiddenOrHasHiddenAncestor && !button.isEnabled
            }

            let startupMarker = "ZMUX_RECONNECT_STARTUP_COMMAND"
            let startup = "printf '\\n\(startupMarker)\\n'; exit 0"
            _ = try await host.nativeCLI([
                "ssh", "fixture", "--ssh-config", config.clientConfig, "--identity", config.identity,
                "--name", "Native reconnect stopped", "--command", startup
            ], timeout: 15_000, externalCaller: true)
            let stoppedWorkspace = try await waitForWorkspace(named: "Native reconnect stopped", snapshot: snapshot)
            createdWorkspaces.append(stoppedWorkspace.id)
            _ = try await host.nativeCLI(["select-workspace", String(stoppedWorkspace.id)], externalCaller: true)

            var targetSurface: Surface?
            var targetSession: UInt64?
            try await waitUntil {
                guard let workspace = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id }),
                      let pane = workspace.root.panes.first,
                      let surface = pane.surfaces.first(where: { $0.kind == "terminal" && $0.remote != nil }),
                      let session = surface.sessionId,
                      let frame = snapshot()?.frames.first(where: { $0.sessionId == session }),
                      frame.reconnectable,
                      frame.snapshot?.viewport.contents.contains(startupMarker) == true,
                      let tab = Self.tab(surface: surface.id, in: host.window.contentView),
                      let button = Self.reconnectButton(in: tab), !button.isHiddenOrHasHiddenAncestor,
                      button.isEnabled, button.toolTip?.lowercased().contains("fresh ssh login") == true else { return false }
                targetSurface = surface
                targetSession = session
                return true
            }
            guard let targetSurface, let targetSession,
                  let pane = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id })?.root.panes.first,
                  pane.selectedSurface == targetSurface.id,
                  let oldView = terminal(targetSession) else {
                throw BridgeFailure(message: "Stopped SSH tab did not mount its native terminal and reconnect icon")
            }

            let targetOrder = pane.surfaces.map(\.id)
            let targetTitle = targetSurface.title
            let targetDestination = targetSurface.remote?.destination
            host.newTerminalInPane(pane.id)
            try await waitUntil {
                guard let updated = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id })?.root.panes.first(where: { $0.id == pane.id }),
                      updated.surfaces.count == targetOrder.count + 1,
                      let live = updated.surfaces.last, let session = live.sessionId,
                      live.remote?.destination == targetDestination,
                      let frame = snapshot()?.frames.first(where: { $0.sessionId == session }),
                      frame.snapshot?.state == "running", !frame.reconnectable,
                      let tab = Self.tab(surface: live.id, in: host.window.contentView),
                      let button = Self.reconnectButton(in: tab) else { return false }
                return button.isHiddenOrHasHiddenAncestor && !button.isEnabled
            }
            guard let paneWithLiveTab = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id })?.root.panes.first(where: { $0.id == pane.id }),
                  paneWithLiveTab.surfaces.first?.id == targetSurface.id,
                  paneWithLiveTab.surfaces.map(\.id).prefix(targetOrder.count).elementsEqual(targetOrder) else {
                throw BridgeFailure(message: "Adding a live SSH tab changed the stopped tab's position")
            }
            host.selectTab(targetSurface.id)
            try await waitUntil { snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id })?.root.panes.first(where: { $0.id == pane.id })?.selectedSurface == targetSurface.id }

            guard let tab = Self.tab(surface: targetSurface.id, in: host.window.contentView),
                  let button = Self.reconnectButton(in: tab), !button.isHiddenOrHasHiddenAncestor, button.isEnabled else {
                throw BridgeFailure(message: "Eligible stopped SSH tab has no enabled native reconnect control")
            }
            try capture(host.window, to: configuration.deletingLastPathComponent().appendingPathComponent("zmux-ssh-reconnect.png"))
            button.performClick(nil)
            guard !button.isEnabled else {
                throw BridgeFailure(message: "Reconnect control remained enabled after its first click")
            }

            var replacementSession: UInt64?
            try await waitUntil {
                guard let updated = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id })?.root.panes.first(where: { $0.id == pane.id }),
                      let replacement = updated.surfaces.first(where: { $0.id == targetSurface.id }),
                      let session = replacement.sessionId, session != targetSession else { return false }
                replacementSession = session
                return true
            }
            guard let replacementSession,
                  let updatedWorkspace = snapshot()?.model.workspaces.first(where: { $0.id == stoppedWorkspace.id }),
                  let updatedPane = updatedWorkspace.root.panes.first(where: { $0.id == pane.id }),
                  let replacement = updatedPane.surfaces.first(where: { $0.id == targetSurface.id }),
                  updatedPane.selectedSurface == targetSurface.id,
                  updatedPane.surfaces.map(\.id) == paneWithLiveTab.surfaces.map(\.id),
                  replacement.title == targetTitle, replacement.remote?.destination == targetDestination,
                  updatedWorkspace.id == stoppedWorkspace.id,
                  snapshot()?.frames.contains(where: { $0.sessionId == targetSession }) == false else {
                throw BridgeFailure(message: "Reconnect did not preserve its tab identity, title, route, pane order and retired session")
            }
            try await waitUntil {
                guard let view = terminal(replacementSession), view !== oldView,
                      view.window === host.window, host.window.firstResponder === view,
                      let frame = snapshot()?.frames.first(where: { $0.sessionId == replacementSession }),
                      let terminalSnapshot = frame.snapshot, terminalSnapshot.state == "running" else { return false }
                return !terminalSnapshot.viewport.contents.contains(startupMarker)
            }
            guard let replacementView = terminal(replacementSession) else {
                throw BridgeFailure(message: "Replacement SSH terminal view disappeared before input")
            }
            replacementView.insertText("printf '\\nZMUX_RECONNECT_NEW_SESSION\\n'\r",
                replacementRange: NSRange(location: NSNotFound, length: 0))
            try await waitUntil {
                snapshot()?.frames.first(where: { $0.sessionId == replacementSession })?.snapshot?.viewport.contents
                    .components(separatedBy: "\n").contains("ZMUX_RECONNECT_NEW_SESSION") == true
            }

            for workspace in createdWorkspaces.reversed() {
                _ = try await host.nativeCLI(["close-workspace", String(workspace)], externalCaller: true)
            }
            createdWorkspaces.removeAll()
            _ = try await host.nativeCLI(["select-workspace", String(originalWorkspace)], externalCaller: true)
            try await waitUntil { snapshot()?.model.focusedWorkspace == originalWorkspace }
        } catch {
            let primary = error
            for workspace in createdWorkspaces.reversed() {
                do { _ = try await host.nativeCLI(["close-workspace", String(workspace)], externalCaller: true) }
                catch { FileHandle.standardError.write(Data("SSH reconnect smoke cleanup failed for workspace \(workspace): \(error.localizedDescription)\n".utf8)) }
            }
            do { _ = try await host.nativeCLI(["select-workspace", String(originalWorkspace)], externalCaller: true) }
            catch { FileHandle.standardError.write(Data("SSH reconnect smoke could not restore the initial workspace: \(error.localizedDescription)\n".utf8)) }
            throw primary
        }
    }

    private static func waitForWorkspace(named name: String, snapshot: @MainActor () -> DesktopSnapshot?) async throws -> Workspace {
        var result: Workspace?
        try await waitUntil {
            result = snapshot()?.model.workspaces.first { $0.name == name }
            return result != nil
        }
        guard let result else { throw BridgeFailure(message: "SSH fixture workspace did not appear: \(name)") }
        return result
    }

    private static func tabs(in view: NSView?) -> [TerminalTabControl] {
        guard let view else { return [] }
        if let tab = view as? TerminalTabControl { return [tab] }
        return view.subviews.flatMap { tabs(in: $0) }
    }

    private static func tab(surface: UInt64, in view: NSView?) -> TerminalTabControl? {
        tabs(in: view).first { $0.surface.id == surface }
    }

    private static func reconnectButton(in tab: TerminalTabControl) -> IconButton? {
        tab.subviews.compactMap { $0 as? IconButton }
            .first { $0.accessibilityLabel()?.lowercased().contains("fresh ssh login") == true }
    }

    private static func capture(_ window: NSWindow, to destination: URL) throws {
        guard let content = window.contentView,
              let image = content.bitmapImageRepForCachingDisplay(in: content.bounds) else {
            throw BridgeFailure(message: "SSH reconnect tab screenshot is unavailable")
        }
        content.layoutSubtreeIfNeeded()
        content.displayIfNeeded()
        content.cacheDisplay(in: content.bounds, to: image)
        guard let png = image.representation(using: .png, properties: [:]), png.count > 4096 else {
            throw BridgeFailure(message: "SSH reconnect tab screenshot is empty")
        }
        try png.write(to: destination, options: .atomic)
    }

    private static func waitUntil(_ predicate: @MainActor () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(25)
        while ContinuousClock.now < deadline {
            if predicate() { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw BridgeFailure(message: "Packaged SSH reconnect native smoke timed out")
    }
}
