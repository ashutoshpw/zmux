import AppKit
import Darwin
import WebKit

/// A generated, private-catalog journey through the packaged CLI, manager and
/// production native owner. No native completion, effect or storage is injected.
@MainActor
enum BrowserProfileAcceptance {
    private struct Prepared: Codable {
        let sourceSHA: String
        let namespace: UUID
        let a: UUID
        let b: UUID
        let c: UUID
        let aName: String
        let bName: String
        let cName: String
        let aSurface: UInt64
        let bSurface: UInt64
        let cSurface: UInt64
        let aIntent: UUID
        let cIntent: UUID
        let origin: String
        let leaseRoot: String
    }
    private static var sourceSHA: String { Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local" }
    private static func stateURL(_ report: URL) -> URL { report.appendingPathExtension("state.json") }
    private static func resultURL(_ report: URL, _ phase: String) -> URL { report.appendingPathExtension(phase + ".json") }
    private static func write(_ object: [String: Any], to url: URL) throws {
        try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]).write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }
    private static func stage(_ name: String, report: URL, phase: String) throws {
        FileHandle.standardError.write(Data("[profile-acceptance pid=\(getpid()) phase=\(phase)] \(name)\n".utf8))
        try write(["phase": phase, "stage": name, "pid": getpid(), "source_sha": sourceSHA], to: report.appendingPathExtension("progress.json"))
    }
    private static func wait(_ message: String, _ predicate: @MainActor () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(25)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func json(_ data: Data) throws -> [String: Any] {
        guard let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw BridgeFailure(message: "Profile fixture expected a JSON object") }
        return value
    }
    private static func cli(_ coordinator: ApplicationCoordinator, _ arguments: [String], failure: Bool = false) async throws -> Data {
        try await coordinator.mainWindowHost.nativeCLI(arguments, expectFailure: failure, timeout: 25000, externalCaller: true)
    }
    private static func eval(_ coordinator: ApplicationCoordinator, _ surface: UInt64, _ script: String) async throws -> Any {
        let result = try json(await cli(coordinator, ["browser", "eval", script, "--surface", String(surface)]))
        guard let value = result["value"] else { throw BridgeFailure(message: "Profile fixture eval omitted its value") }
        return value
    }
    private static func requireValue(_ coordinator: ApplicationCoordinator, _ surface: UInt64, _ script: String, _ message: String) async throws {
        guard try await eval(coordinator, surface, script) as? Bool == true else { throw BridgeFailure(message: message) }
    }
    private static func seed(_ coordinator: ApplicationCoordinator, _ surface: UInt64, _ value: String) async throws {
        _ = try await eval(coordinator, surface, "localStorage.setItem('profileFixture', '\(value)'); document.cookie = 'profileFixture=\(value); Max-Age=3600; Path=/'; true")
        try await requireData(coordinator, surface, value)
    }
    private static func requireData(_ coordinator: ApplicationCoordinator, _ surface: UInt64, _ value: String) async throws {
        try await requireValue(coordinator, surface, "localStorage.getItem('profileFixture') === '\(value)' && document.cookie.includes('profileFixture=\(value)')", "Profile fixture lost its own cookie/localStorage value \(value)")
    }
    private static func requireEmpty(_ coordinator: ApplicationCoordinator, _ surface: UInt64) async throws {
        try await requireValue(coordinator, surface, "localStorage.getItem('profileFixture') === null && !document.cookie.includes('profileFixture=')", "Cleared profile retained its fixture cookie/localStorage")
    }
    private static func open(_ coordinator: ApplicationCoordinator, _ profile: UUID, origin: String, hidden: Bool = false) async throws -> UInt64 {
        let result = try json(await cli(coordinator, ["browser", "open", "--profile", profile.uuidString] + (hidden ? ["--no-focus"] : []) + [origin + "/profile"]))
        guard let surfaces = result["created_surfaces"] as? [[String: Any]], let id = (surfaces.first?["surface_id"] as? NSNumber)?.uint64Value else { throw BridgeFailure(message: "Profile open omitted its surface") }
        try await loaded(coordinator, id, profile: profile)
        return id
    }
    private static func loaded(_ coordinator: ApplicationCoordinator, _ surface: UInt64, profile: UUID) async throws {
        try await wait("Profile UUID \(profile) did not load in its actual WebKit view") {
            guard let view = coordinator.browsers[surface]?.webView else { return false }
            return view.title == "Profile acceptance" && !view.isLoading && view.configuration.websiteDataStore.identifier == profile
        }
    }
    private static func close(_ coordinator: ApplicationCoordinator, _ surface: UInt64) async throws {
        _ = try await cli(coordinator, ["close-surface", "--surface", String(surface)])
        try await wait("Closed profile retained its model or native view") {
            coordinator.browsers[surface] == nil && coordinator.snapshot?.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { $0.id == surface } == false
        }
    }
    private static func add(_ coordinator: ApplicationCoordinator, _ name: String) async throws -> UUID {
        let result = try json(await cli(coordinator, ["browser", "profiles", "add", name]))
        guard let raw = result["id"] as? String, let id = UUID(uuidString: raw) else { throw BridgeFailure(message: "Profile add omitted UUID") }
        return id
    }
    private static func catalog(_ coordinator: ApplicationCoordinator) throws -> [BrowserProfile] {
        guard let state = coordinator.snapshot?.browserProfiles, state.enabled, !state.loading, state.error == nil else { throw BridgeFailure(message: "Profile fixture catalog unavailable") }
        return state.profiles
    }
    private static func refuse(_ coordinator: ApplicationCoordinator, _ profile: UUID, _ reason: String) async throws {
        let before = try catalog(coordinator)
        let data = try await cli(coordinator, ["browser", "profiles", "clear", profile.uuidString], failure: true)
        let text = String(decoding: data, as: UTF8.self).lowercased()
        guard text.contains("busy"), try catalog(coordinator) == before else { throw BridgeFailure(message: "\(reason) refusal changed the catalog or lacked a scoped busy result: \(text)") }
    }
    private static func original(_ coordinator: ApplicationCoordinator, profile: UUID, intent: UUID) async throws {
        let value = try json(await cli(coordinator, ["browser", "profiles", "status", profile.uuidString, intent.uuidString]))
        guard let record = value["record"] as? [String: Any], record["phase"] as? String == "completed",
              (record["profile"] as? String)?.lowercased() == profile.uuidString.lowercased(),
              (record["intent"] as? String)?.lowercased() == intent.uuidString.lowercased(),
              value["outcome_unknown"] as? Bool == false, value["physical_cleanup_pending"] as? Bool == false else {
            throw BridgeFailure(message: "Original intent lacks durable terminal/physical cleanup proof: \(value)")
        }
    }
    private static func clear(_ coordinator: ApplicationCoordinator, profile: UUID, delete: Bool = false) async throws -> UUID {
        let result = try json(await cli(coordinator, ["browser", "profiles", delete ? "delete" : "clear", profile.uuidString]))
        guard let raw = result["intent"] as? String, let intent = UUID(uuidString: raw), result["state"] as? String == "completed" else { throw BridgeFailure(message: "Profile mutation did not return its completed original intent: \(result)") }
        try await original(coordinator, profile: profile, intent: intent)
        return intent
    }
    private static func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
    private static func manager() throws -> NSWindow {
        guard let item = NSApp.mainMenu?.items.flatMap({ $0.submenu?.items ?? [] }).first(where: { $0.action == NSSelectorFromString("showBrowserProfiles:") }), let menu = item.menu else { throw BridgeFailure(message: "Actual profile manager menu unavailable") }
        menu.performActionForItem(at: menu.index(of: item))
        guard let window = NSApp.windows.first(where: { $0.title == "Browser Profiles" && $0.isVisible }) else { throw BridgeFailure(message: "Actual profile manager window unavailable") }
        return window
    }
    private static func button(_ title: String, in window: NSWindow) throws -> NSButton {
        guard let root = window.contentView, let button = descendants(root).compactMap({ $0 as? NSButton }).first(where: { $0.title == title }), button.isEnabled else { throw BridgeFailure(message: "Actual enabled manager button unavailable: \(title)") }
        return button
    }
    private static func select(_ profile: UUID, in manager: NSWindow) throws {
        guard let root = manager.contentView, let popup = descendants(root).compactMap({ $0 as? NSPopUpButton }).first,
              let item = popup.itemArray.first(where: { ($0.representedObject as? String)?.lowercased() == profile.uuidString.lowercased() }),
              let action = popup.action else { throw BridgeFailure(message: "Manager omitted generated profile UUID") }
        popup.select(item)
        NSApp.sendAction(action, to: popup.target, from: popup)
    }
    private static func managerMutation(_ coordinator: ApplicationCoordinator, profile: UUID, delete: Bool = false, cancel: Bool = false) async throws -> UUID? {
        let window = try manager()
        try select(profile, in: window)
        let prior = Set(coordinator.snapshot?.browserProfiles.operations.map(\.operationId) ?? [])
        let before = try catalog(coordinator)
        try button(delete ? "Delete Profile…" : "Clear Website Data…", in: window).performClick(nil)
        try await wait("Manager did not present its actual destructive confirmation sheet") { window.attachedSheet != nil }
        guard let sheet = window.attachedSheet else { throw BridgeFailure(message: "Manager confirmation sheet disappeared") }
        try button(cancel ? "Cancel" : (delete ? "Delete Profile" : "Clear Website Data"), in: sheet).performClick(nil)
        try await wait("Manager confirmation sheet did not close") { window.attachedSheet == nil }
        if cancel {
            guard try catalog(coordinator) == before, Set(coordinator.snapshot?.browserProfiles.operations.map(\.operationId) ?? []) == prior else { throw BridgeFailure(message: "Cancelled manager confirmation admitted an operation") }
            return nil
        }
        try await wait("Manager destructive operation did not finish") {
            coordinator.snapshot?.browserProfiles.operations.contains { !prior.contains($0.operationId) && $0.profile?.id == profile && $0.state != "pending" } == true
        }
        guard let receipt = coordinator.snapshot?.browserProfiles.operations.first(where: { !prior.contains($0.operationId) && $0.profile?.id == profile }), receipt.state == "completed", let intent = receipt.intent else { throw BridgeFailure(message: "Manager mutation failed rather than completing original intent") }
        try await original(coordinator, profile: profile, intent: intent)
        try await wait("Manager completion feedback unavailable") {
            guard let root = window.contentView else { return false }
            return descendants(root).compactMap { $0 as? NSTextField }.contains { $0.accessibilityLabel() == "Profile status" && $0.stringValue == (delete ? "Profile deleted." : "Website data cleared.") }
        }
        return intent
    }
    private static func managerReconcile() async throws {
        let window = try manager()
        try button("Finish Interrupted Operation", in: window).performClick(nil)
        try await wait("Manager did not confirm original operation reconciliation") {
            guard let root = window.contentView else { return false }
            return descendants(root).compactMap { $0 as? NSTextField }.contains { $0.accessibilityLabel() == "Profile status" && $0.stringValue == "Original operation completed." }
        }
    }
    private static func foreignBusy(_ coordinator: ApplicationCoordinator, profile: UUID, report: URL) async throws {
        let executable = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("native-profile-lease-holder")
        let lifetime = URL(fileURLWithPath: try BrowserProfileLeaseDirectory.resolve()).appendingPathComponent("browser-profile-" + profile.uuidString.lowercased() + ".lock")
        let ready = report.appendingPathExtension("foreign-ready")
        let process = Process(), input = Pipe()
        process.executableURL = executable
        process.arguments = [lifetime.path, ready.path]
        process.standardInput = input
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        do {
            try await wait("Independent profile owner did not acquire its shared lifetime inode") { FileManager.default.fileExists(atPath: ready.path) || !process.isRunning }
            guard process.isRunning, FileManager.default.fileExists(atPath: ready.path) else { throw BridgeFailure(message: "Independent profile owner exited before admission") }
            try await refuse(coordinator, profile, "Independent-process lifetime owner")
            guard process.isRunning else { throw BridgeFailure(message: "Foreign owner exited during refusal") }
        } catch {
            try input.fileHandleForWriting.close()
            await Task.detached { process.waitUntilExit() }.value
            throw error
        }
        try input.fileHandleForWriting.close()
        await Task.detached { process.waitUntilExit() }.value
        guard process.terminationReason == .exit, process.terminationStatus == 0 else { throw BridgeFailure(message: "Independent profile owner did not exit cleanly") }
    }
    private static func downloadBusy(_ coordinator: ApplicationCoordinator, profile: UUID, surface: UInt64, report: URL) async throws {
        let window = coordinator.mainWindowHost.window
        let prior = Set(coordinator.snapshot?.downloads.map(\.downloadId) ?? [])
        _ = try await eval(coordinator, surface, "const a = document.createElement('a'); a.href = '/download'; a.download = 'profile-fixture.bin'; document.body.append(a); a.click(); true")
        try await wait("Actual WebKit download conversion and Save sheet were not observed") {
            window.attachedSheet is NSSavePanel && coordinator.snapshot?.downloads.contains { !prior.contains($0.downloadId) && !$0.terminal } == true
        }
        do {
            try await refuse(coordinator, profile, "Actual WebKit conversion/download owner with live tab")
            try await requireData(coordinator, surface, "ownerA")
        } catch {
            (window.attachedSheet as? NSSavePanel)?.cancel(nil)
            throw error
        }
        (window.attachedSheet as? NSSavePanel)?.cancel(nil)
        try await wait("Actual download cancellation did not confirm physical cleanup") {
            coordinator.snapshot?.downloads.contains { !prior.contains($0.downloadId) && $0.status == "cancelled" && $0.cleanupComplete } == true && !coordinator.browserDownloads.isProfileBusy(profile)
        }
        try stage("actual download conversion/refusal/cancel cleanup; tab guard also present", report: report, phase: "prepare")
    }
    static func run(coordinator: ApplicationCoordinator, report: URL, phase: String, origin: String) async throws {
        try stage("waiting for production startup/restore", report: report, phase: phase)
        try await wait("Profile acceptance startup/restore did not settle") {
            guard let snapshot = coordinator.snapshot else { return false }
            return snapshot.persistence?.startupPending != true && snapshot.persistence?.restoring != true && !snapshot.browserProfiles.loading && snapshot.browserProfiles.enabled && coordinator.mainWindowHost.nativeReady
        }
        if phase == "prepare" { try await prepare(coordinator, report: report, origin: origin) }
        else {
            let saved = try JSONDecoder().decode(Prepared.self, from: Data(contentsOf: stateURL(report)))
            guard saved.sourceSHA == sourceSHA, saved.origin == origin, coordinator.snapshot?.model.identityNamespace == saved.namespace else { throw BridgeFailure(message: "Fresh reader changed source, fixture origin or persisted namespace") }
            if phase == "reader" { try await reader(coordinator, report: report, saved: saved) }
            else if phase == "verify" { try await verify(coordinator, report: report, saved: saved) }
            else { throw BridgeFailure(message: "Unknown profile acceptance phase") }
        }
    }
    private static func prepare(_ coordinator: ApplicationCoordinator, report: URL, origin: String) async throws {
        guard try catalog(coordinator).isEmpty else { throw BridgeFailure(message: "Refusing profile acceptance against a nonempty catalog") }
        let suffix = UUID().uuidString, aName = "Acceptance A " + UUID().uuidString, bName = "Acceptance B " + UUID().uuidString
        let cName = "Acceptance C " + suffix
        let a = try await add(coordinator, aName), b = try await add(coordinator, bName), c = try await add(coordinator, cName)
        try stage("generated A/B/C UUIDs; mounted and hidden busy refusals", report: report, phase: "prepare")
        let aMounted = try await open(coordinator, a, origin: origin)
        guard coordinator.browsers[aMounted]?.webView.window != nil else { throw BridgeFailure(message: "Mounted refusal fixture has no actual window") }
        try await seed(coordinator, aMounted, "ownerA")
        try await refuse(coordinator, a, "Mounted profile")
        try await requireData(coordinator, aMounted, "ownerA")
        let bSurface = try await open(coordinator, b, origin: origin)
        try await seed(coordinator, bSurface, "ownerB")
        let cFirst = try await open(coordinator, c, origin: origin, hidden: true)
        try await seed(coordinator, cFirst, "ownerC")
        guard coordinator.snapshot?.workspace?.root.panes.contains(where: { $0.surfaces.contains { $0.id == cFirst } && $0.selectedSurface != cFirst }) == true else { throw BridgeFailure(message: "Hidden refusal fixture was selected") }
        try await refuse(coordinator, c, "Hidden profile")
        try await requireData(coordinator, cFirst, "ownerC")
        try await downloadBusy(coordinator, profile: a, surface: aMounted, report: report)
        try await close(coordinator, aMounted)
        try await foreignBusy(coordinator, profile: a, report: report)
        let aAfterForeign = try await open(coordinator, a, origin: origin)
        try await requireData(coordinator, aAfterForeign, "ownerA")
        try await close(coordinator, aAfterForeign)
        try stage("CLI clear A and real manager clear C", report: report, phase: "prepare")
        let firstAIntent = try await clear(coordinator, profile: a)
        let aAfterClear = try await open(coordinator, a, origin: origin)
        try await requireEmpty(coordinator, aAfterClear)
        _ = try await managerMutation(coordinator, profile: c, cancel: true)
        try await requireData(coordinator, cFirst, "ownerC")
        try await close(coordinator, cFirst)
        guard let firstCIntent = try await managerMutation(coordinator, profile: c) else { throw BridgeFailure(message: "Manager clear omitted intent") }
        let cAfterClear = try await open(coordinator, c, origin: origin)
        try await requireEmpty(coordinator, cAfterClear)
        try await requireData(coordinator, bSurface, "ownerB")
        try stage("connected original-intent CLI/manager reconciliation preserves post-clear data", report: report, phase: "prepare")
        try await seed(coordinator, aAfterClear, "afterClearA")
        try await seed(coordinator, cAfterClear, "afterClearC")
        _ = try await cli(coordinator, ["browser", "profiles", "reconcile", a.uuidString, firstAIntent.uuidString])
        try await managerReconcile()
        try await original(coordinator, profile: c, intent: firstCIntent)
        try await requireData(coordinator, aAfterClear, "afterClearA")
        try await requireData(coordinator, cAfterClear, "afterClearC")
        try await close(coordinator, aAfterClear)
        try await close(coordinator, cAfterClear)
        // Leave a directly observable empty store for an independent fresh reader.
        let aIntent = try await clear(coordinator, profile: a), cIntent = try await clear(coordinator, profile: c)
        let aSurface = try await open(coordinator, a, origin: origin), cSurface = try await open(coordinator, c, origin: origin)
        try await requireEmpty(coordinator, aSurface)
        try await requireEmpty(coordinator, cSurface)
        try await requireData(coordinator, bSurface, "ownerB")
        guard let namespace = coordinator.snapshot?.model.identityNamespace,
              Set(try catalog(coordinator).map(\.id)) == Set([a, b, c]) else { throw BridgeFailure(message: "Clear changed immutable catalog membership") }
        let saved = Prepared(sourceSHA: sourceSHA, namespace: namespace, a: a, b: b, c: c, aName: aName, bName: bName, cName: cName, aSurface: aSurface, bSurface: bSurface, cSurface: cSurface, aIntent: aIntent, cIntent: cIntent, origin: origin, leaseRoot: try BrowserProfileLeaseDirectory.resolve())
        try JSONEncoder().encode(saved).write(to: stateURL(report), options: .atomic)
        try write(["source_sha": sourceSHA, "pid": getpid(), "success": false, "prepared": true, "mounted_busy": true, "hidden_busy": true, "foreign_busy": true, "download_conversion_with_live_tab_busy": true, "download_cancel_cleanup": true, "confirmation_cancel": true, "cli_clear": true, "manager_clear": true, "cli_reconciliation_no_replay": true, "manager_reconciliation_no_replay": true, "untouched_b": true, "shutdown_complete": false], to: resultURL(report, "prepare"))
        try stage("prepared; awaiting physical application shutdown", report: report, phase: "prepare")
    }
    private static func reader(_ coordinator: ApplicationCoordinator, report: URL, saved: Prepared) async throws {
        try stage("fresh reader checks restored A/C empty and B retained without reseeding", report: report, phase: "reader")
        guard Set(try catalog(coordinator).map(\.id)) == Set([saved.a, saved.b, saved.c]),
              try catalog(coordinator).contains(BrowserProfile(id: saved.a, name: saved.aName)),
              try catalog(coordinator).contains(BrowserProfile(id: saved.c, name: saved.cName)) else { throw BridgeFailure(message: "Clear changed catalog UUID/name after restart") }
        for (surface, profile) in [(saved.aSurface, saved.a), (saved.bSurface, saved.b), (saved.cSurface, saved.c)] { try await loaded(coordinator, surface, profile: profile) }
        try await requireEmpty(coordinator, saved.aSurface)
        try await requireEmpty(coordinator, saved.cSurface)
        try await requireData(coordinator, saved.bSurface, "ownerB")
        try await original(coordinator, profile: saved.a, intent: saved.aIntent)
        try await original(coordinator, profile: saved.c, intent: saved.cIntent)
        _ = try await cli(coordinator, ["browser", "profiles", "reconcile", saved.a.uuidString, saved.aIntent.uuidString])
        try await close(coordinator, saved.aSurface)
        try await close(coordinator, saved.cSurface)
        try stage("actual CLI delete C and manager delete A", report: report, phase: "reader")
        let cDelete = try await clear(coordinator, profile: saved.c, delete: true)
        guard let aDelete = try await managerMutation(coordinator, profile: saved.a, delete: true) else { throw BridgeFailure(message: "Manager delete omitted original intent") }
        guard try catalog(coordinator) == [BrowserProfile(id: saved.b, name: saved.bName)] else { throw BridgeFailure(message: "Delete changed unrelated B membership") }
        try await original(coordinator, profile: saved.a, intent: aDelete)
        try await original(coordinator, profile: saved.c, intent: cDelete)
        _ = try await cli(coordinator, ["browser", "profiles", "reconcile", saved.c.uuidString, cDelete.uuidString])
        try await managerReconcile()
        try await requireData(coordinator, saved.bSurface, "ownerB")
        try write(["source_sha": sourceSHA, "pid": getpid(), "success": false, "fresh_restore": true, "fresh_a_empty": true, "fresh_c_empty": true, "fresh_b_retained": true, "restarted_terminal_reconciliation": true, "cli_delete": true, "manager_delete": true, "deleted_original_status": true, "deleted_original_reconciliation": true, "a_delete_intent": aDelete.uuidString, "c_delete_intent": cDelete.uuidString, "shutdown_complete": false], to: resultURL(report, "reader"))
    }
    private static func verify(_ coordinator: ApplicationCoordinator, report: URL, saved: Prepared) async throws {
        try stage("fresh deletion reader and public generated UUID cleanup", report: report, phase: "verify")
        guard try catalog(coordinator) == [BrowserProfile(id: saved.b, name: saved.bName)],
              coordinator.snapshot?.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }).contains(where: { $0.browserProfileId == saved.a || $0.browserProfileId == saved.c }) == false else { throw BridgeFailure(message: "Deleted profile membership/surface returned after restart") }
        try await loaded(coordinator, saved.bSurface, profile: saved.b)
        try await requireData(coordinator, saved.bSurface, "ownerB")
        let prior = try json(Data(contentsOf: resultURL(report, "reader")))
        for (profile, key) in [(saved.a, "a_delete_intent"), (saved.c, "c_delete_intent")] {
            guard let raw = prior[key] as? String, let intent = UUID(uuidString: raw) else { throw BridgeFailure(message: "Deleted original intent missing from generated receipt") }
            try await original(coordinator, profile: profile, intent: intent)
            _ = try await cli(coordinator, ["browser", "profiles", "reconcile", profile.uuidString, intent.uuidString])
            _ = try await cli(coordinator, ["browser", "open", "--profile", profile.uuidString, saved.origin + "/profile"], failure: true)
        }
        try await requireData(coordinator, saved.bSurface, "ownerB")
        try await close(coordinator, saved.bSurface)
        _ = try await clear(coordinator, profile: saved.b, delete: true)
        guard try catalog(coordinator).isEmpty else { throw BridgeFailure(message: "Generated profile public cleanup left catalog membership") }
        try write(["source_sha": sourceSHA, "pid": getpid(), "success": false, "fresh_deleted_membership": true, "fresh_deleted_status": true, "fresh_deleted_reconciliation": true, "untouched_b_retained": true, "all_generated_uuid_cleanup": true, "shutdown_complete": false], to: resultURL(report, "verify"))
    }
    static func shutdownComplete(report: URL, phase: String) throws {
        let url = resultURL(report, phase)
        var receipt = try json(Data(contentsOf: url))
        receipt["shutdown_complete"] = true
        receipt["success"] = true
        try write(receipt, to: url)
        try stage("shared shutdown completed; runner must still wait/reap original PID", report: report, phase: phase)
    }
}
