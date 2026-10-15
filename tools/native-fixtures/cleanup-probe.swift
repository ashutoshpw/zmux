import Foundation

@MainActor
final class FixtureOwner {
    private nonisolated static func removeExtracted(_ fixture: URL) async throws {
        let cleanup = Task<Void, Error>.detached {
            precondition(!Thread.isMainThread, "Filesystem work ran on the UI thread")
            try FileManager.default.removeItem(at: fixture)
        }
        try await cleanup.value
    }

    func remove(_ fixture: URL) async throws {
#if NESTED
        func removeFixture() async throws {
            let cleanup = Task<Void, Error>.detached {
                precondition(!Thread.isMainThread, "Filesystem work ran on the UI thread")
                try FileManager.default.removeItem(at: fixture)
            }
            try await cleanup.value
        }
        try await removeFixture()
#else
        try await Self.removeExtracted(fixture)
#endif
    }
}

@main
struct FixtureCleanupProbe {
    @MainActor
    static func main() async throws {
        let setup = Task<URL, Error>.detached {
            precondition(!Thread.isMainThread)
            let root = FileManager.default.temporaryDirectory
                .appendingPathComponent("zmux-cleanup-probe-" + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
                                                   attributes: [.posixPermissions: 0o700])
            return root
        }
        let root = try await setup.value
        let owner = FixtureOwner()
        try await owner.remove(root)
        let removed = Task<Bool, Never>.detached { !FileManager.default.fileExists(atPath: root.path) }
        guard await removed.value else { fatalError("Cleanup returned before removal") }
        do {
            try await owner.remove(root)
            fatalError("Removal failure was swallowed")
        } catch {
            print("Cleanup awaited off-main removal and propagated the missing-path failure.")
        }
    }
}
