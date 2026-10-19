import CryptoKit
import Foundation
import Security

struct CloudDeviceIdentityPublic: Sendable {
    let installationID: UUID
    let publicKey: Data
}

/// The endpoint-scoped identity for this installation. Its secret and agreement
/// key stay in ThisDeviceOnly Keychain and are never returned to UI callers.
struct CloudDeviceIdentity: Sendable {
    static let keychainService = "app.w3dev.zmux.dev.cloud.device-identity.v1"

    private struct Record: Codable {
        let version: Int
        let installationID: UUID
        let deviceSecret: Data
        let privateKey: Data
    }

    let publicIdentity: CloudDeviceIdentityPublic
    let deviceSecret: Data
    let privateKey: Data

    var deviceSecretHeader: String { deviceSecret.cloudBase64URLEncoded() }

    init(endpoint: CloudEndpoint) throws {
        let record = try Self.loadOrCreate(endpoint: endpoint)
        guard record.version == 1, record.deviceSecret.count == 32, record.privateKey.count == 32 else {
            throw CloudFailure(message: "This Mac's cloud device identity is invalid. Remove its Keychain item before registering again.")
        }
        let agreementKey: Curve25519.KeyAgreement.PrivateKey
        do { agreementKey = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: record.privateKey) }
        catch { throw CloudFailure(message: "This Mac's cloud encryption key is invalid.") }
        publicIdentity = CloudDeviceIdentityPublic(installationID: record.installationID,
                                                   publicKey: agreementKey.publicKey.rawRepresentation)
        deviceSecret = record.deviceSecret
        privateKey = record.privateKey
    }

    static func keychainQuery(endpoint: CloudEndpoint) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: keychainService,
         kSecAttrAccount as String: endpoint.origin.absoluteString]
    }

    private static func loadOrCreate(endpoint: CloudEndpoint) throws -> Record {
        let query = keychainQuery(endpoint: endpoint)
        for _ in 0..<3 {
            var read = query
            read[kSecReturnData as String] = true
            read[kSecMatchLimit as String] = kSecMatchLimitOne
            var item: CFTypeRef?
            let status = SecItemCopyMatching(read as CFDictionary, &item)
            if status == errSecSuccess, let data = item as? Data {
                do { return try JSONDecoder().decode(Record.self, from: data) }
                catch { throw CloudFailure(message: "Unable to decode this Mac's cloud device identity.") }
            }
            guard status == errSecItemNotFound else {
                throw CloudFailure(message: "Unable to read this Mac's cloud device identity from Keychain (\(status)).")
            }

            let key = Curve25519.KeyAgreement.PrivateKey()
            let record = Record(version: 1, installationID: UUID(), deviceSecret: try Self.randomBytes(count: 32),
                                privateKey: key.rawRepresentation)
            var add = query
            add[kSecValueData as String] = try JSONEncoder().encode(record)
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let added = SecItemAdd(add as CFDictionary, nil)
            if added == errSecSuccess { return record }
            // Another CloudClient/process may have created the same item after
            // our read. Reload it; never overwrite a persisted installation ID.
            if added != errSecDuplicateItem {
                throw CloudFailure(message: "Unable to save this Mac's cloud device identity in Keychain (\(added)).")
            }
        }
        throw CloudFailure(message: "Unable to load this Mac's cloud device identity after a concurrent Keychain update.")
    }

    private static func randomBytes(count: Int) throws -> Data {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            throw CloudFailure(message: "Unable to generate secure cloud device credentials.")
        }
        return Data(bytes)
    }
}

extension Data {
    func cloudBase64URLEncoded() -> String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    static func cloudBase64URLDecoded(_ value: String, maximumBytes: Int) -> Data? {
        guard value.utf8.count <= maximumBytes * 4 / 3 + 8,
              value.utf8.allSatisfy({
                  ($0 >= 65 && $0 <= 90) || ($0 >= 97 && $0 <= 122) || ($0 >= 48 && $0 <= 57) || $0 == 45 || $0 == 95
              }) else { return nil }
        var normalized = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        let remainder = normalized.utf8.count % 4
        if remainder != 0 { normalized += String(repeating: "=", count: 4 - remainder) }
        guard let data = Data(base64Encoded: normalized), data.count <= maximumBytes else { return nil }
        return data
    }
}
