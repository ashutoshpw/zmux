import CryptoKit
import Darwin
import Foundation

/// Developer opt-in only. The builder owns archive admission; this adapter
/// resolves and rechecks its final bundled paths without acquiring from PATH.
struct NativeLocalZellijBundle: Sendable {
    let configuration: NativeLocalZellijConfiguration
    let profileID: UUID
    let profileRoot: URL
    let bundleRoot: URL
    let sourceSHA: String
    let manifestSHA256: String
    let binarySHA256: String
    let cliSHA256: String

    private struct Metadata: Decodable {
        struct Dependency: Decodable {
            let target: String
            let variant: String?
            let diagnosticOnly: Bool?
            let manifestSha256: String
            let binarySha256: String
            let inventorySha256: String?
            let protocolSha256: String?
            let nativeStartAttachIntegrated: Bool
            let defaultAcquisitionChanged: Bool
        }
        let sourceSha: String
        let architectures: [String]
        let defaultTerminalEngine: String
        let optionalZellijPackaged: Bool
        let optionalZellij: Dependency?
    }
    private struct Manifest: Decodable {
        let schemaVersion: Int
        let promotionReady: Bool
        let variant: String
        let diagnosticOnly: Bool
        let binarySha256: String
    }

    static func resolve(bundle: URL, profile: UUID, root: URL) async throws -> NativeLocalZellijBundle {
        try await Task.detached {
            let bundle = bundle.resolvingSymlinksInPath().standardizedFileURL
            let root = root.standardizedFileURL
            guard root.path.hasPrefix("/"), root == root.resolvingSymlinksInPath(),
                  profile != UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)) else {
                throw BridgeFailure(message: "Explicit local Zellij profile requires a nonzero ID and canonical absolute root")
            }
            var info = stat()
            guard lstat(root.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
                  info.st_uid == geteuid(), info.st_mode & 0o077 == 0 else {
                throw BridgeFailure(message: "Explicit local Zellij profile must be an owned private real directory")
            }
            let resources = bundle.appendingPathComponent("Contents/Resources", isDirectory: true)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let metadata = try decoder.decode(Metadata.self, from: bytes(resources.appendingPathComponent("build-info.json"), limit: 65_536))
            guard metadata.architectures == ["arm64"], metadata.defaultTerminalEngine == "vt100",
                  metadata.optionalZellijPackaged, let admitted = metadata.optionalZellij,
                  admitted.target == "aarch64-apple-darwin", admitted.variant == "input-readiness-v1",
                  admitted.diagnosticOnly == true, admitted.nativeStartAttachIntegrated,
                  !admitted.defaultAcquisitionChanged,
                  [admitted.manifestSha256, admitted.binarySha256, admitted.inventorySha256 ?? "", admitted.protocolSha256 ?? ""].allSatisfy(isDigest) else {
                throw BridgeFailure(message: "Native local Zellij opt-in requires the explicit admitted diagnostic readiness bundle")
            }
            let dependency = resources.appendingPathComponent("Dependencies/local-zellij", isDirectory: true)
            let manifestBytes = try bytes(dependency.appendingPathComponent("manifest.json"), limit: 8 * 1024 * 1024)
            let manifest = try decoder.decode(Manifest.self, from: manifestBytes)
            guard sha256(manifestBytes) == admitted.manifestSha256, manifest.schemaVersion == 1,
                  manifest.promotionReady, manifest.variant == "input-readiness-v1", manifest.diagnosticOnly,
                  manifest.binarySha256 == admitted.binarySha256 else {
                throw BridgeFailure(message: "Final bundled local Zellij manifest differs from admission")
            }
            let cli = resources.appendingPathComponent("bin/zmux")
            let zellij = dependency.appendingPathComponent("bin/zellij")
            guard cli == cli.resolvingSymlinksInPath(), zellij == zellij.resolvingSymlinksInPath(),
                  FileManager.default.isExecutableFile(atPath: cli.path), FileManager.default.isExecutableFile(atPath: zellij.path) else {
                throw BridgeFailure(message: "Native local Zellij requires real executable paths inside the admitted bundle")
            }
            let binary = try bytes(zellij, limit: 64 * 1024 * 1024)
            guard sha256(binary) == admitted.binarySha256 else {
                throw BridgeFailure(message: "Final bundled local Zellij executable differs from admission")
            }
            return NativeLocalZellijBundle(configuration: NativeLocalZellijConfiguration(
                installation: .init(epoch: UUID(), cli: cli.path, zellij: zellij.path),
                profiles: [.init(id: profile, root: root.path)]), profileID: profile, profileRoot: root,
                bundleRoot: bundle, sourceSHA: metadata.sourceSha, manifestSHA256: admitted.manifestSha256,
                binarySHA256: admitted.binarySha256, cliSHA256: sha256(try bytes(cli, limit: 64 * 1024 * 1024)))
        }.value
    }

    static func bytes(_ url: URL, limit: Int) throws -> Data {
        let descriptor = open(url.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK)
        guard descriptor >= 0 else { throw BridgeFailure(message: "Cannot open bounded local Zellij input: " + url.lastPathComponent) }
        let file = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? file.close() }
        var info = stat()
        guard fstat(descriptor, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_size >= 0, info.st_size <= Int64(limit) else {
            throw BridgeFailure(message: "Local Zellij input is nonregular or exceeds its byte bound")
        }
        var data = Data()
        while let chunk = try file.read(upToCount: min(65_536, limit + 1 - data.count)), !chunk.isEmpty {
            data.append(chunk)
            guard data.count <= limit else { throw BridgeFailure(message: "Local Zellij input grew beyond its byte bound") }
        }
        return data
    }
    private static func isDigest(_ text: String) -> Bool {
        text.utf8.count == 64 && text.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    private static func sha256(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
}
