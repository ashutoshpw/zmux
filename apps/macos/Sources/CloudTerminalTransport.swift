import CryptoKit
import Foundation

struct CloudTerminalClaim: Sendable, Equatable {
    let id: UUID
    let senderDeviceID: UUID
    let senderPublicKey: Data
    let expiresAt: Date
    let targetEpoch: UUID
    /// Decrypted request bytes. The host decodes its narrow typed operation DTO.
    let payload: Data
}

struct CloudTerminalCryptoContext: Sendable {
    let requestID: UUID
    let senderDeviceID: UUID
    let targetDeviceID: UUID
    let targetEpoch: UUID
}

enum CloudTerminalDirection: String, Sendable {
    case request
    case response
}

struct CloudTerminalOpenedPayload: Sendable {
    let payload: Data
    let issuedAt: Date
    let expiresAt: Date
}

/// End-to-end envelope codec. CloudClient remains the authenticated network owner.
enum CloudTerminalTransport {
    static let maximumPlaintextBytes = 256 * 1024
    static let maximumEncodedEnvelopeCharacters = 350_000
    private static let maximumHeaderBytes = 1_024
    private static let maximumCombinedBytes = maximumPlaintextBytes + 28
    private static let maximumFrameBytes = 4 + maximumHeaderBytes + maximumCombinedBytes
    private static let maximumLifetimeMilliseconds: Int64 = 20_000
    private static let allowedClockSkewMilliseconds: Int64 = 5_000
    private static let protocolVersion = 1
    private static let domain = "zmux.cloud.terminal.v1"

    private struct Header: Codable {
        let version: Int
        let requestId: UUID
        let senderDeviceId: UUID
        let targetDeviceId: UUID
        let targetEpoch: UUID
        let issuedAtMilliseconds: Int64
        let expiresAtMilliseconds: Int64
    }

    static func seal(_ plaintext: Data, using identity: CloudDeviceIdentity,
                     peerPublicKey: Data, context: CloudTerminalCryptoContext,
                     direction: CloudTerminalDirection, issuedAt: Date, expiresAt: Date) throws -> String {
        guard plaintext.count <= maximumPlaintextBytes else {
            throw CloudFailure(message: "Terminal request exceeded the 256 KiB limit.")
        }
        let issuedMilliseconds = try milliseconds(issuedAt)
        let expiryMilliseconds = try milliseconds(expiresAt)
        try validateTimes(issuedAtMilliseconds: issuedMilliseconds, expiresAtMilliseconds: expiryMilliseconds, now: Date())
        let authenticatedData = aad(context: context, direction: direction,
                                    issuedAtMilliseconds: issuedMilliseconds, expiresAtMilliseconds: expiryMilliseconds)
        let key = try deriveKey(identity: identity, peerPublicKey: peerPublicKey, info: authenticatedData)
        let sealed: ChaChaPoly.SealedBox
        do { sealed = try ChaChaPoly.seal(plaintext, using: key, authenticating: authenticatedData) }
        catch { throw CloudFailure(message: "Unable to encrypt the terminal request.") }
        let combined = sealed.combined
        guard combined.count <= maximumCombinedBytes else {
            throw CloudFailure(message: "Terminal request exceeded the encrypted payload limit.")
        }
        let header = Header(version: protocolVersion, requestId: context.requestID,
                            senderDeviceId: context.senderDeviceID, targetDeviceId: context.targetDeviceID,
                            targetEpoch: context.targetEpoch, issuedAtMilliseconds: issuedMilliseconds,
                            expiresAtMilliseconds: expiryMilliseconds)
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let headerData = try encoder.encode(header)
        guard !headerData.isEmpty, headerData.count <= maximumHeaderBytes else {
            throw CloudFailure(message: "Terminal request metadata exceeded the relay envelope limit.")
        }
        var frame = Data()
        var headerLength = UInt32(headerData.count).bigEndian
        Swift.withUnsafeBytes(of: &headerLength) { frame.append(contentsOf: $0) }
        frame.append(headerData)
        frame.append(combined)
        guard frame.count <= maximumFrameBytes else {
            throw CloudFailure(message: "Terminal request exceeded the relay envelope limit.")
        }
        let encoded = frame.cloudBase64URLEncoded()
        guard encoded.utf8.count <= maximumEncodedEnvelopeCharacters else {
            throw CloudFailure(message: "Terminal request exceeded the relay envelope limit.")
        }
        return encoded
    }

    static func open(_ encoded: String, using identity: CloudDeviceIdentity,
                     peerPublicKey: Data, expected context: CloudTerminalCryptoContext,
                     direction: CloudTerminalDirection, serverExpiresAt: Date? = nil) throws -> CloudTerminalOpenedPayload {
        guard encoded.utf8.count <= maximumEncodedEnvelopeCharacters,
              let frame = Data.cloudBase64URLDecoded(encoded, maximumBytes: maximumFrameBytes),
              frame.count >= 4 + 1 + 28 else {
            throw CloudFailure(message: "The terminal relay returned an invalid encrypted envelope.")
        }
        let headerLength = (Int(frame[0]) << 24) | (Int(frame[1]) << 16) | (Int(frame[2]) << 8) | Int(frame[3])
        guard headerLength > 0, headerLength <= maximumHeaderBytes,
              headerLength <= frame.count - 4 else {
            throw CloudFailure(message: "The terminal relay returned invalid metadata framing.")
        }
        let headerStart = 4
        let messageStart = headerStart + headerLength
        let headerData = frame.subdata(in: headerStart..<messageStart)
        let combined = frame.subdata(in: messageStart..<frame.count)
        guard combined.count >= 28, combined.count <= maximumCombinedBytes else {
            throw CloudFailure(message: "The terminal relay returned a malformed encrypted message.")
        }
        let header: Header
        do {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            header = try decoder.decode(Header.self, from: headerData)
        } catch { throw CloudFailure(message: "The terminal relay returned an incompatible encrypted envelope.") }

        guard header.version == protocolVersion,
              header.requestId == context.requestID,
              header.senderDeviceId == context.senderDeviceID,
              header.targetDeviceId == context.targetDeviceID,
              header.targetEpoch == context.targetEpoch else {
            throw CloudFailure(message: "The terminal relay request did not match its authenticated context.")
        }
        try validateTimes(issuedAtMilliseconds: header.issuedAtMilliseconds,
                          expiresAtMilliseconds: header.expiresAtMilliseconds, now: Date())
        let issuedAt = Date(timeIntervalSince1970: Double(header.issuedAtMilliseconds) / 1_000)
        let expiresAt = Date(timeIntervalSince1970: Double(header.expiresAtMilliseconds) / 1_000)
        if let serverExpiresAt, expiresAt.timeIntervalSince(serverExpiresAt) > 1 {
            throw CloudFailure(message: "The terminal relay envelope outlives its server request.")
        }
        let authenticatedData = aad(context: context, direction: direction,
                                    issuedAtMilliseconds: header.issuedAtMilliseconds,
                                    expiresAtMilliseconds: header.expiresAtMilliseconds)
        let key = try deriveKey(identity: identity, peerPublicKey: peerPublicKey, info: authenticatedData)
        do {
            let box = try ChaChaPoly.SealedBox(combined: combined)
            let plaintext = try ChaChaPoly.open(box, using: key, authenticating: authenticatedData)
            guard plaintext.count <= maximumPlaintextBytes else {
                throw CloudFailure(message: "Terminal payload exceeded the 256 KiB limit.")
            }
            return CloudTerminalOpenedPayload(payload: plaintext, issuedAt: issuedAt, expiresAt: expiresAt)
        } catch let failure as CloudFailure { throw failure }
        catch { throw CloudFailure(message: "The terminal relay message failed authentication.") }
    }

    private static func deriveKey(identity: CloudDeviceIdentity, peerPublicKey: Data, info: Data) throws -> SymmetricKey {
        guard peerPublicKey.count == 32 else { throw CloudFailure(message: "The peer device has an invalid encryption key.") }
        do {
            let local = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: identity.privateKey)
            let peer = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: peerPublicKey)
            let shared = try local.sharedSecretFromKeyAgreement(with: peer)
            return shared.hkdfDerivedSymmetricKey(using: SHA256.self, salt: Data(domain.utf8),
                                                  sharedInfo: info, outputByteCount: 32)
        } catch let failure as CloudFailure { throw failure }
        catch { throw CloudFailure(message: "Unable to establish the encrypted terminal channel.") }
    }

    private static func aad(context: CloudTerminalCryptoContext, direction: CloudTerminalDirection,
                            issuedAtMilliseconds: Int64, expiresAtMilliseconds: Int64) -> Data {
        var result = Data()
        for component in [domain, String(protocolVersion), context.requestID.uuidString.lowercased(),
                          context.senderDeviceID.uuidString.lowercased(), context.targetDeviceID.uuidString.lowercased(),
                          context.targetEpoch.uuidString.lowercased(), direction.rawValue,
                          String(issuedAtMilliseconds), String(expiresAtMilliseconds)] {
            append(component, to: &result)
        }
        return result
    }

    private static func append(_ value: String, to data: inout Data) {
        let bytes = Data(value.utf8)
        var length = UInt32(bytes.count).bigEndian
        Swift.withUnsafeBytes(of: &length) { data.append(contentsOf: $0) }
        data.append(bytes)
    }

    private static func milliseconds(_ date: Date) throws -> Int64 {
        let timestamp = date.timeIntervalSince1970 * 1_000
        guard timestamp.isFinite, timestamp >= Double(Int64.min), timestamp < Double(Int64.max) else {
            throw CloudFailure(message: "The terminal relay message has an invalid timestamp.", outcomeUnknown: true)
        }
        return Int64(timestamp.rounded(.down))
    }

    private static func validateTimes(issuedAtMilliseconds: Int64, expiresAtMilliseconds: Int64,
                                      now: Date) throws {
        let nowMilliseconds = try milliseconds(now)
        let issueWindow = maximumLifetimeMilliseconds + allowedClockSkewMilliseconds
        let (earliestIssue, issueUnderflow) = nowMilliseconds.subtractingReportingOverflow(issueWindow)
        let (latestIssue, issueOverflow) = nowMilliseconds.addingReportingOverflow(issueWindow)
        let (earliestExpiry, expiryUnderflow) = nowMilliseconds.subtractingReportingOverflow(allowedClockSkewMilliseconds)
        let (latestExpiry, lifetimeOverflow) = issuedAtMilliseconds.addingReportingOverflow(maximumLifetimeMilliseconds)
        guard !issueUnderflow, !issueOverflow, !expiryUnderflow, !lifetimeOverflow,
              issuedAtMilliseconds >= earliestIssue, issuedAtMilliseconds <= latestIssue,
              expiresAtMilliseconds > issuedAtMilliseconds, expiresAtMilliseconds <= latestExpiry,
              expiresAtMilliseconds > earliestExpiry else {
            throw CloudFailure(message: "The terminal relay message has expired or has an invalid lifetime.", outcomeUnknown: true)
        }
    }
}
