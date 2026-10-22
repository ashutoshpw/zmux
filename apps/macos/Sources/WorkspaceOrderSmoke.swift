import AppKit

/// The bundled CLI, real workspace-row menus and the native sidebar drop path
/// must reconcile the same order while live content remains in place.
@MainActor
enum WorkspaceOrderSmoke {
    private static let dragType = NSPasteboard.PasteboardType("app.zmux.workspace-order")

    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }

    private static func selection(_ snapshot: DesktopSnapshot?) -> [UInt64] {
        guard let model = snapshot?.model else { return [] }
        var result = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows { result += [window.id, window.selectedWorkspace ?? 0] }
        for workspace in model.workspaces {
            result += [workspace.id, workspace.focusedPane]
            for pane in workspace.root.panes { result += [pane.id, pane.selectedSurface] }
        }
        return result
    }

    static func validate(
        window: NSWindow,
        sidebar: WorkspaceSidebarList,
        report: URL,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        rows: @MainActor () -> [WorkspaceRowView],
        terminals: @MainActor () -> [UInt64: TerminalView],
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        // Settle earlier fixture cleanup before capturing the live identities.
        let status = try await cli(["status"], false)
        guard let current = try JSONSerialization.jsonObject(with: status) as? [String: Any],
              let revision = current["model_revision"] as? UInt64 else {
            throw BridgeFailure(message: "Workspace order fixture could not observe its executor revision")
        }
        try await wait("Workspace order fixture did not observe prior cleanup") { (snapshot()?.model.revision ?? 0) >= revision }
        guard let initial = snapshot(), let owner = initial.model.windows.first(where: { $0.id == initial.model.focusedWindow }),
              owner.workspaceIds.count >= 2, let subject = owner.workspaceIds.first,
              let responder = window.firstResponder, window.isKeyWindow else {
            throw BridgeFailure(message: "Workspace order fixture needs two workspaces and a real content responder")
        }
        let original = owner.workspaceIds
        let selected = selection(initial)
        let terminalViews = terminals().mapValues { ObjectIdentifier($0) }
        let processes = Dictionary(uniqueKeysWithValues: initial.frames.map { ($0.sessionId, $0.processId) })
        let browserIDs = initial.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }
            .filter { $0.kind == "browser" }.map(\.id)
        let browserViews = browserIDs.compactMap { id -> (UInt64, ObjectIdentifier, UUID, UInt64)? in
            guard let view = browser(id) else { return nil }
            return (id, ObjectIdentifier(view), view.hostID, view.documentGeneration)
        }
        guard browserViews.count == browserIDs.count, !browserViews.isEmpty else {
            throw BridgeFailure(message: "Workspace order fixture lost its real browser host")
        }
        let scope = ["--window", String(owner.id)]
        @MainActor func unchanged() throws {
            guard selection(snapshot()) == selected, window.isKeyWindow, NSApp.keyWindow === window,
                  window.firstResponder === responder, terminals().mapValues({ ObjectIdentifier($0) }) == terminalViews,
                  Dictionary(uniqueKeysWithValues: (snapshot()?.frames ?? []).map { ($0.sessionId, $0.processId) }) == processes,
                  browserViews.allSatisfy({ id, identity, host, generation in
                      guard let view = browser(id) else { return false }
                      return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                  }) else {
                throw BridgeFailure(message: "Reorder changed selection, key/responder, a live process or native content identity")
            }
        }
        @MainActor func settled(_ expected: [UInt64]) async throws {
            try await wait("Workspace order did not reach the actual sidebar rows") {
                snapshot()?.model.windows.first(where: { $0.id == owner.id })?.workspaceIds == expected && rows().map(\.workspaceID) == expected
            }
            try unchanged()
        }

        let preview = try await cli(["reorder-workspace", String(subject), "--after", String(subject), "--dry-run",
                                     "--expect-revision", String(initial.model.revision)] + scope, false)
        guard let receipt = try JSONSerialization.jsonObject(with: preview) as? [String: Any],
              receipt["to_index"] as? Int == 1, receipt["dry_run"] as? Bool == true,
              snapshot()?.model.revision == initial.model.revision else {
            throw BridgeFailure(message: "Workspace preview mutated the model or lost after-self behavior")
        }
        try unchanged()
        _ = try await cli(["reorder-workspace", String(subject), "--after", String(subject)] + scope, false)
        var advanced = original
        advanced.removeFirst()
        advanced.insert(subject, at: 1)
        try await settled(advanced)
        guard let menu = rows().first(where: { $0.workspaceID == subject })?.menu,
              let item = menu.items.first(where: { $0.title == "Move to Top" }), item.isEnabled else {
            throw BridgeFailure(message: "Workspace row does not expose its real enabled reorder menu")
        }
        menu.performActionForItem(at: menu.index(of: item))
        try await settled(original)
        let reversed = Array(original.reversed())
        _ = try await cli(["reorder-workspaces", "--order", reversed.map(String.init).joined(separator: ",")] + scope, false)
        try await settled(reversed)
        _ = try await cli(["reorder-workspaces", "--order", original.map(String.init).joined(separator: ",")] + scope, false)
        try await settled(original)

        try await validateSidebarDrag(window: window, sidebar: sidebar, report: report, cli: cli,
                                      snapshot: snapshot, rows: rows, terminals: terminals, browser: browser,
                                      browserViews: browserViews, original: original, windowID: owner.id,
                                      selectedWorkspace: owner.selectedWorkspace ?? subject)
        FileHandle.standardError.write(Data("Native workspace reorder: CLI preview/apply/bulk, real row menu and sidebar drag callbacks, canonical order and live focus/content retention passed\n".utf8))
    }

    private static func validateSidebarDrag(
        window: NSWindow,
        sidebar: WorkspaceSidebarList,
        report: URL,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        rows: @MainActor () -> [WorkspaceRowView],
        terminals: @MainActor () -> [UInt64: TerminalView],
        browser: @MainActor (UInt64) -> BrowserView?,
        browserViews: [(UInt64, ObjectIdentifier, UUID, UInt64)],
        original: [UInt64],
        windowID: UInt64,
        selectedWorkspace: UInt64
    ) async throws {
        guard let before = snapshot(), let owner = before.model.windows.first(where: { $0.id == windowID }),
              owner.workspaceIds == original, let responder = window.firstResponder, window.isKeyWindow,
              original.count >= 2,
              !before.model.workspaceGroups.contains(where: { $0.windowId == windowID }) else {
            throw BridgeFailure(message: "Workspace drag fixture needs an ungrouped, focused two-workspace window")
        }
        let beforeSelection = selection(before)
        let beforeTerminalViews = terminals().mapValues { ObjectIdentifier($0) }
        let beforeProcesses = Dictionary(uniqueKeysWithValues: before.frames.map { ($0.sessionId, $0.processId) })

        let scope = ["--window", String(windowID)]
        let firstMembers = Array(original.prefix(2))
        let workspaceName = "Workspace drag smoke \(UUID().uuidString)"
        let groupName = "Workspace drag group \(UUID().uuidString)"
        var temporaryID: UInt64?
        let scrollView = sidebar.enclosingScrollView
        let clipView = scrollView?.contentView
        let savedClipOrigin = clipView?.bounds.origin

        @MainActor func cleanup() async throws {
            let group = snapshot()?.model.workspaceGroups.first { $0.windowId == windowID && $0.workspaceIds == firstMembers }
            if let group { _ = try await cli(["workspace-group", "ungroup", String(group.id)] + scope, false) }
            let fixtureID = temporaryID ?? snapshot()?.model.workspaces.first(where: { $0.name == workspaceName })?.id
            if let fixtureID, snapshot()?.model.workspaces.contains(where: { $0.id == fixtureID }) == true {
                _ = try await cli(["close-workspace"] + scope + [String(fixtureID)], false)
            }
            if snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds != original {
                _ = try await cli(["reorder-workspaces", "--order", original.map(String.init).joined(separator: ",")] + scope, false)
            }
            if snapshot()?.model.focusedWorkspace != selectedWorkspace {
                _ = try await cli(["select-workspace", String(selectedWorkspace)] + scope, false)
            }
            try await wait("Workspace drag fixture did not restore its original order and selection") {
                snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds == original &&
                    snapshot()?.model.focusedWorkspace == selectedWorkspace &&
                    snapshot()?.model.workspaceGroups.contains(where: { $0.windowId == windowID && $0.workspaceIds == firstMembers }) == false &&
                    rows().map(\.workspaceID) == original
            }
            if let clipView, let savedClipOrigin {
                clipView.scroll(to: savedClipOrigin)
                scrollView?.reflectScrolledClipView(clipView)
            }
            guard window.makeFirstResponder(responder), window.firstResponder === responder,
                  selection(snapshot()) == beforeSelection,
                  terminals().mapValues({ ObjectIdentifier($0) }) == beforeTerminalViews,
                  Dictionary(uniqueKeysWithValues: (snapshot()?.frames ?? []).map { ($0.sessionId, $0.processId) }) == beforeProcesses,
                  browserViews.allSatisfy({ id, identity, host, generation in
                      guard let view = browser(id) else { return false }
                      return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                  }) else {
                throw BridgeFailure(message: "Workspace drag fixture cleanup did not restore focus or live content")
            }
        }

        do {
            _ = try await cli(["new-workspace", workspaceName], false)
            try await wait("Workspace drag fixture did not create its temporary workspace") {
                snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds.count == original.count + 1
            }
            guard let created = snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds.last,
                  !original.contains(created) else {
                throw BridgeFailure(message: "Workspace drag fixture could not identify its temporary workspace")
            }
            temporaryID = created
            try await wait("Temporary workspace terminal was not attached") {
                guard let state = snapshot(),
                      let workspace = state.model.workspaces.first(where: { $0.id == created }),
                      let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }),
                      let sessionID = pane.sessionId,
                      terminals()[sessionID] != nil,
                      let frame = state.frames.first(where: { $0.sessionId == sessionID }) else { return false }
                return frame.processId != nil && frame.snapshot?.state == "running"
            }

            _ = try await cli(["workspace-group", "create", "--name", groupName,
                               "--from", firstMembers.map(String.init).joined(separator: ",")] + scope, false)
            try await wait("Workspace drag fixture did not group the first two workspaces") {
                snapshot()?.model.workspaceGroups.contains { $0.windowId == windowID && $0.workspaceIds == firstMembers } == true &&
                    rows().map(\.workspaceID) == original + [created]
            }
            guard let groupID = snapshot()?.model.workspaceGroups.first(where: { $0.windowId == windowID && $0.workspaceIds == firstMembers })?.id else {
                throw BridgeFailure(message: "Workspace drag fixture could not identify its temporary group")
            }
            guard snapshot()?.model.focusedWorkspace == created else {
                throw BridgeFailure(message: "Grouping unexpectedly changed the temporary fixture selection")
            }

            // A real row click must leave selection alone on mouse-down and
            // select only when the same row receives mouse-up.
            guard let clickID = original.first(where: { $0 != selectedWorkspace }),
                  let clickRow = rows().first(where: { $0.workspaceID == clickID }) else {
                throw BridgeFailure(message: "Workspace drag fixture lacks an unselected real row")
            }
            let clickDownSelection = selection(snapshot())
            guard let clickDownResponder = window.firstResponder else {
                throw BridgeFailure(message: "Workspace drag fixture lost its post-create content responder")
            }
            clickRow.mouseDown(with: try mouseEvent(.leftMouseDown, window: window, view: clickRow))
            guard selection(snapshot()) == clickDownSelection, window.firstResponder === clickDownResponder else {
                throw BridgeFailure(message: "Workspace row mouse-down selected or focused before a drag threshold")
            }
            clickRow.mouseUp(with: try mouseEvent(.leftMouseUp, window: window, view: clickRow))
            try await wait("Workspace row mouse-up did not select its workspace") { snapshot()?.model.focusedWorkspace == clickID }
            guard let restoreRow = rows().first(where: { $0.workspaceID == selectedWorkspace }) else {
                throw BridgeFailure(message: "Workspace drag fixture could not restore its selected row")
            }
            restoreRow.mouseDown(with: try mouseEvent(.leftMouseDown, window: window, view: restoreRow))
            guard snapshot()?.model.focusedWorkspace == clickID else {
                throw BridgeFailure(message: "Workspace row mouse-down changed selection during click restoration")
            }
            restoreRow.mouseUp(with: try mouseEvent(.leftMouseUp, window: window, view: restoreRow))
            try await wait("Workspace drag fixture could not restore its selected workspace") {
                snapshot()?.model.focusedWorkspace == selectedWorkspace
            }
            guard window.makeFirstResponder(responder), window.firstResponder === responder else {
                throw BridgeFailure(message: "Workspace row click did not permit restoring the original content responder")
            }

            guard let dragBaseline = snapshot(), window.isKeyWindow, NSApp.keyWindow === window else {
                throw BridgeFailure(message: "Workspace drag fixture lost its key window")
            }
            let dragSelection = selection(dragBaseline)
            let dragResponder = window.firstResponder
            let dragTerminalViews = terminals().mapValues { ObjectIdentifier($0) }
            let dragProcesses = Dictionary(uniqueKeysWithValues: dragBaseline.frames.map { ($0.sessionId, $0.processId) })
            @MainActor func unchangedDrag() throws {
                guard selection(snapshot()) == dragSelection, window.isKeyWindow, NSApp.keyWindow === window,
                      window.firstResponder === dragResponder,
                      terminals().mapValues({ ObjectIdentifier($0) }) == dragTerminalViews,
                      Dictionary(uniqueKeysWithValues: (snapshot()?.frames ?? []).map { ($0.sessionId, $0.processId) }) == dragProcesses,
                      browserViews.allSatisfy({ id, identity, host, generation in
                          guard let view = browser(id) else { return false }
                          return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                      }) else {
                    throw BridgeFailure(message: "Sidebar drag changed selection, key/responder, a live process or browser identity")
                }
            }
            @MainActor func layoutSidebar() {
                window.contentView?.layoutSubtreeIfNeeded()
                sidebar.layoutSubtreeIfNeeded()
            }
            @MainActor func settledDrag(_ expected: [UInt64]) async throws {
                try await wait("Sidebar drag did not reconcile canonical and rendered workspace order") {
                    snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds == expected &&
                        rows().map(\.workspaceID) == expected
                }
                layoutSidebar()
                try unchangedDrag()
            }
            @MainActor func verifyGroup(_ order: [UInt64]) throws {
                guard snapshot()?.model.workspaceGroups.contains(where: { $0.windowId == windowID && $0.workspaceIds == firstMembers }) == true,
                      let first = order.firstIndex(of: firstMembers[0]),
                      first + firstMembers.count <= order.count,
                      order[first..<(first + firstMembers.count)].elementsEqual(firstMembers) else {
                    throw BridgeFailure(message: "Sidebar drag changed the group anchor, membership or contiguity")
                }
            }
            @MainActor func beginDrag(_ workspaceID: UInt64) throws -> (WorkspaceRowView, WorkspaceOrderTarget) {
                guard let source = rows().first(where: { $0.workspaceID == workspaceID }) else {
                    throw BridgeFailure(message: "Sidebar drag source row disappeared")
                }
                source.mouseDown(with: try mouseEvent(.leftMouseDown, window: window, view: source))
                guard let target = source.activeDragTarget else { throw BridgeFailure(message: "Workspace row did not capture a drag target") }
                try unchangedDrag()
                return (source, target)
            }

            // The drag callback receives window-base coordinates, so convert a
            // real row/header point through the flipped sidebar and its clip view.
            layoutSidebar()
            guard sidebar.isFlipped, let clipView, let scrollView, let savedClipOrigin else {
                throw BridgeFailure(message: "Workspace drag fixture has no flipped, scrolled sidebar")
            }
            clipView.setBoundsOrigin(NSPoint(x: savedClipOrigin.x, y: savedClipOrigin.y + 8))
            scrollView.reflectScrolledClipView(clipView)
            guard abs(clipView.bounds.origin.y - savedClipOrigin.y) >= 1 else {
                throw BridgeFailure(message: "Workspace drag fixture could not exercise a nonzero sidebar scroll offset")
            }
            guard let header = sidebar.arrangedSubviews.compactMap({ $0 as? WorkspaceGroupRowView })
                    .first(where: { $0.groupID == groupID }),
                  let anchorRow = rows().first(where: { $0.workspaceID == firstMembers[0] }),
                  let fixtureID = temporaryID else {
                throw BridgeFailure(message: "Workspace drag fixture lost its grouped destination")
            }
            @MainActor func verifyGroupStartCue(at point: NSPoint, source: WorkspaceRowView, target: WorkspaceOrderTarget, label: String) throws {
                let info = try dragInfo(source: source, target: target, sidebar: sidebar, point: point)
                guard sidebar.draggingEntered(info).contains(.move), let cue = insertionCue(in: sidebar),
                      abs(cue.frame.midY - header.frame.minY) < 1 else {
                    throw BridgeFailure(message: "Workspace drag \(label) did not target the group-start insertion cue")
                }
                try unchangedDrag()
                sidebar.draggingExited(nil)
            }
            let upperHeaderPoint = NSPoint(x: header.frame.midX, y: header.frame.midY - 1)
            let lowerHeaderPoint = NSPoint(x: header.frame.midX, y: header.frame.maxY - 1)
            let upperAnchorPoint = NSPoint(x: anchorRow.frame.midX, y: anchorRow.frame.minY + 1)
            let (firstSource, firstTarget) = try beginDrag(fixtureID)
            try verifyGroupStartCue(at: upperHeaderPoint, source: firstSource, target: firstTarget, label: "upper-header hover")
            try verifyGroupStartCue(at: lowerHeaderPoint, source: firstSource, target: firstTarget, label: "lower-header hover")
            try verifyGroupStartCue(at: upperAnchorPoint, source: firstSource, target: firstTarget, label: "upper-anchor hover")
            let beforeGroupInfo = try dragInfo(source: firstSource, target: firstTarget, sidebar: sidebar, point: upperAnchorPoint)
            guard sidebar.draggingEntered(beforeGroupInfo).contains(.move), insertionCue(in: sidebar) != nil else {
                throw BridgeFailure(message: "Valid workspace hover did not show the group-start insertion cue")
            }
            try captureInsertionCue(in: sidebar, to: report.deletingLastPathComponent().appendingPathComponent("zmux-workspace-drag.png"))
            guard sidebar.prepareForDragOperation(beforeGroupInfo), sidebar.performDragOperation(beforeGroupInfo) else {
                throw BridgeFailure(message: "Valid workspace drag was not admitted by the real sidebar callback")
            }
            firstSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: firstSource))
            let beforeGroupOrder = [fixtureID] + original
            try await settledDrag(beforeGroupOrder)
            try verifyGroup(beforeGroupOrder)

            guard let lastMember = rows().first(where: { $0.workspaceID == firstMembers[1] }) else {
                throw BridgeFailure(message: "Workspace drag fixture lost the final grouped member")
            }
            let afterGroupPoint: NSPoint
            if let next = rows().first(where: { $0.workspaceID == original.dropFirst(2).first }) {
                afterGroupPoint = NSPoint(x: next.frame.midX, y: next.frame.minY + 1)
            } else {
                afterGroupPoint = NSPoint(x: lastMember.frame.midX, y: lastMember.frame.maxY + 1)
            }
            let (secondSource, secondTarget) = try beginDrag(fixtureID)
            let afterGroupInfo = try dragInfo(source: secondSource, target: secondTarget, sidebar: sidebar, point: afterGroupPoint)
            guard sidebar.draggingEntered(afterGroupInfo).contains(.move), sidebar.prepareForDragOperation(afterGroupInfo),
                  sidebar.performDragOperation(afterGroupInfo) else {
                throw BridgeFailure(message: "Workspace drag after the group boundary was not admitted")
            }
            secondSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: secondSource))
            let afterGroupOrder = Array(original.prefix(2)) + [fixtureID] + Array(original.dropFirst(2))
            try await settledDrag(afterGroupOrder)
            try verifyGroup(afterGroupOrder)

            // A same-slot hover and a gap inside the anchored group are both
            // rejected before the drop callback can enqueue a model mutation.
            let (noopSource, noopTarget) = try beginDrag(fixtureID)
            let noopInfo = try dragInfo(source: noopSource, target: noopTarget, sidebar: sidebar,
                                        point: NSPoint(x: noopSource.frame.midX, y: noopSource.frame.maxY + 1))
            try rejected(noopInfo, in: sidebar, message: "Workspace sidebar offered a no-op insertion")
            noopSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: noopSource))

            guard let secondMemberRow = rows().first(where: { $0.workspaceID == firstMembers[1] }) else {
                throw BridgeFailure(message: "Workspace drag fixture lost its group-splitting destination")
            }
            let (splitSource, splitTarget) = try beginDrag(fixtureID)
            let splitInfo = try dragInfo(source: splitSource, target: splitTarget, sidebar: sidebar,
                                         point: NSPoint(x: secondMemberRow.frame.midX, y: secondMemberRow.frame.minY + 1))
            try rejected(splitInfo, in: sidebar, message: "Workspace sidebar allowed a drop that splits a workspace group")
            splitSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: splitSource))

            let (staleSource, staleTarget) = try beginDrag(fixtureID)
            let stale = WorkspaceOrderTarget(namespace: staleTarget.namespace, workspace: staleTarget.workspace,
                                             window: staleTarget.window, index: staleTarget.index,
                                             revision: staleTarget.revision == 0 ? 1 : staleTarget.revision - 1)
            let staleInfo = try dragInfo(source: staleSource, target: staleTarget, sidebar: sidebar,
                                         point: upperHeaderPoint, payloadTarget: stale)
            try rejected(staleInfo, in: sidebar, message: "Workspace sidebar accepted a mismatched drag payload")
            staleSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: staleSource))

            let (foreignSource, foreignTarget) = try beginDrag(fixtureID)
            let validForeignInfo = try dragInfo(source: foreignSource, target: foreignTarget, sidebar: sidebar,
                                                point: upperHeaderPoint)
            guard let originalOwner = foreignSource.workspaceDragOwner else {
                throw BridgeFailure(message: "Workspace row lost its live drag owner")
            }
            let foreignOwner = NSObject()
            foreignSource.workspaceDragOwner = foreignOwner
            try rejected(validForeignInfo, in: sidebar, message: "Workspace sidebar accepted a foreign-owner drag source")
            foreignSource.workspaceDragOwner = originalOwner
            foreignSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: foreignSource))
            try unchangedDrag()

            // A revision change alone rejects a still-attached source before
            // the row rebuilds; a real collapse below also invalidates its row.
            let (revisionSource, revisionTarget) = try beginDrag(fixtureID)
            let revisionInfo = try dragInfo(source: revisionSource, target: revisionTarget,
                                            sidebar: sidebar, point: upperHeaderPoint)
            guard let currentOrderState = sidebar.orderState, let currentState = currentOrderState() else {
                throw BridgeFailure(message: "Workspace sidebar lost its revision-bound order state")
            }
            sidebar.orderState = {
                WorkspaceSidebarOrderState(namespace: currentState.namespace, window: currentState.window,
                    revision: currentState.revision &+ 1, workspaceIDs: currentState.workspaceIDs,
                    workspaceUUIDs: currentState.workspaceUUIDs, groups: currentState.groups)
            }
            do { try rejected(revisionInfo, in: sidebar, message: "Workspace sidebar accepted a stale source revision") }
            catch {
                sidebar.orderState = currentOrderState
                throw error
            }
            sidebar.orderState = currentOrderState
            revisionSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: revisionSource))

            // A real CLI collapse advances the revision after this source has
            // captured its token. A fresh source can target the collapsed header.
            let (oldRevisionSource, oldRevisionTarget) = try beginDrag(fixtureID)
            let oldRevisionInfo = try dragInfo(source: oldRevisionSource, target: oldRevisionTarget,
                                               sidebar: sidebar, point: upperHeaderPoint)
            _ = try await cli(["workspace-group", "collapse", String(groupID)] + scope, false)
            let collapsedVisibleRows = [fixtureID] + Array(original.dropFirst(2))
            try await wait("Workspace drag fixture did not collapse its group") {
                snapshot()?.model.workspaceGroups.first(where: { $0.id == groupID })?.collapsed == true &&
                    rows().map(\.workspaceID) == collapsedVisibleRows &&
                    sidebar.arrangedSubviews.compactMap({ $0 as? WorkspaceGroupRowView })
                        .contains(where: { $0.groupID == groupID && !$0.isHidden })
            }
            layoutSidebar()
            try rejected(oldRevisionInfo, in: sidebar, message: "Workspace sidebar accepted a source captured before the revision changed")
            oldRevisionSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: oldRevisionSource))

            guard let collapsedHeader = sidebar.arrangedSubviews.compactMap({ $0 as? WorkspaceGroupRowView })
                    .first(where: { $0.groupID == groupID }) else {
                throw BridgeFailure(message: "Workspace drag fixture lost its collapsed group header")
            }
            let (collapsedSource, collapsedTarget) = try beginDrag(fixtureID)
            let collapsedHeaderPoint = NSPoint(x: collapsedHeader.frame.midX, y: collapsedHeader.frame.midY - 1)
            let collapsedInfo = try dragInfo(source: collapsedSource, target: collapsedTarget,
                                             sidebar: sidebar, point: collapsedHeaderPoint)
            guard sidebar.draggingEntered(collapsedInfo).contains(.move), let collapsedCue = insertionCue(in: sidebar),
                  abs(collapsedCue.frame.midY - collapsedHeader.frame.minY) < 1,
                  sidebar.prepareForDragOperation(collapsedInfo), sidebar.performDragOperation(collapsedInfo) else {
                throw BridgeFailure(message: "Fresh workspace drag could not target the collapsed group boundary")
            }
            collapsedSource.mouseUp(with: try mouseEvent(.rightMouseUp, window: window, view: collapsedSource))
            let collapsedOrder = [fixtureID] + original
            let collapsedRows = [fixtureID] + Array(original.dropFirst(2))
            try await wait("Collapsed group drop did not preserve canonical and rendered order") {
                snapshot()?.model.windows.first(where: { $0.id == windowID })?.workspaceIds == collapsedOrder &&
                    rows().map(\.workspaceID) == collapsedRows &&
                    snapshot()?.model.workspaceGroups.first(where: { $0.id == groupID })?.collapsed == true
            }
            try unchangedDrag()
            try verifyGroup(collapsedOrder)
            _ = try await cli(["workspace-group", "expand", String(groupID)] + scope, false)
            try await wait("Workspace drag fixture could not expand the group after its collapsed-header drop") {
                snapshot()?.model.workspaceGroups.first(where: { $0.id == groupID })?.collapsed == false &&
                    rows().map(\.workspaceID) == collapsedOrder
            }
            try unchangedDrag()
            try verifyGroup(collapsedOrder)

            try await cleanup()
        } catch {
            do { try await cleanup() } catch {
                FileHandle.standardError.write(Data("Workspace drag fixture cleanup also failed: \(error.localizedDescription)\n".utf8))
            }
            throw error
        }
    }

    private static func mouseEvent(_ type: NSEvent.EventType, window: NSWindow, view: NSView) throws -> NSEvent {
        guard let event = NSEvent.mouseEvent(
            with: type,
            location: view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil),
            modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber,
            context: nil,
            eventNumber: 0,
            clickCount: 1,
            pressure: 1
        ) else { throw BridgeFailure(message: "Could not synthesize a native workspace-row mouse event") }
        return event
    }

    private static func dragInfo(
        source: WorkspaceRowView,
        target: WorkspaceOrderTarget,
        sidebar: WorkspaceSidebarList,
        point: NSPoint,
        payloadTarget: WorkspaceOrderTarget? = nil
    ) throws -> WorkspaceOrderDraggingInfo {
        let pasteboard = NSPasteboard(name: NSPasteboard.Name("zmux.workspace-drag-\(UUID().uuidString)"))
        pasteboard.clearContents()
        guard let payload = try? JSONEncoder().encode(payloadTarget ?? target),
              payload.count <= 512, pasteboard.setData(payload, forType: dragType) else {
            throw BridgeFailure(message: "Could not encode the bounded workspace drag token")
        }
        let windowPoint = sidebar.convert(point, to: nil)
        return WorkspaceOrderDraggingInfo(source: source, location: windowPoint, pasteboard: pasteboard)
    }

    private static func insertionCue(in sidebar: WorkspaceSidebarList) -> NSView? {
        sidebar.subviews.first { !$0.isHidden && abs($0.frame.height - 2) < 0.5 }
    }

    private static func rejected(_ info: WorkspaceOrderDraggingInfo, in sidebar: WorkspaceSidebarList, message: String) throws {
        guard sidebar.draggingEntered(info).isEmpty,
              !sidebar.prepareForDragOperation(info), !sidebar.performDragOperation(info),
              insertionCue(in: sidebar) == nil else {
            throw BridgeFailure(message: message)
        }
    }

    private static func captureInsertionCue(in sidebar: WorkspaceSidebarList, to url: URL) throws {
        sidebar.layoutSubtreeIfNeeded()
        sidebar.displayIfNeeded()
        guard let bitmap = sidebar.bitmapImageRepForCachingDisplay(in: sidebar.bounds) else {
            throw BridgeFailure(message: "Workspace drag insertion cue could not be rendered")
        }
        sidebar.cacheDisplay(in: sidebar.bounds, to: bitmap)
        guard let png = bitmap.representation(using: .png, properties: [:]), png.count > 1024 else {
            throw BridgeFailure(message: "Workspace drag insertion cue did not produce a usable image")
        }
        try png.write(to: url, options: .atomic)
    }
}

/// Supplies only the AppKit drag data needed by WorkspaceSidebarList's real
/// destination callbacks; it does not claim to synthesize an OS drag session.
@MainActor
private final class WorkspaceOrderDraggingInfo: NSObject, NSDraggingInfo {
    let draggingLocation: NSPoint
    let draggingPasteboard: NSPasteboard
    let draggingSource: Any?

    var animatesToDestination = false
    var draggingDestinationWindow: NSWindow? { nil }
    var draggingFormation: NSDraggingFormation = .default
    var draggingSequenceNumber = 1
    var draggingSourceOperationMask: NSDragOperation = .move
    var draggedImageLocation: NSPoint = .zero
    var numberOfValidItemsForDrop = 1
    var springLoadingHighlight: NSSpringLoadingHighlight = .none

    nonisolated var draggedImage: NSImage? { nil }
    nonisolated override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }

    init(source: WorkspaceRowView, location: NSPoint, pasteboard: NSPasteboard) {
        draggingSource = source
        draggingLocation = location
        draggingPasteboard = pasteboard
        super.init()
    }

    func enumerateDraggingItems(
        options enumOpts: NSDraggingItemEnumerationOptions,
        for view: NSView?,
        classes classArray: [AnyClass],
        searchOptions: [NSPasteboard.ReadingOptionKey: Any],
        using block: @escaping (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void
    ) {}

    func resetSpringLoading() {}
    func slideDraggedImage(to screenPoint: NSPoint) {}
}
