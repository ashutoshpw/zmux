import Darwin
import Foundation

/// Real shared-socket mutation for native projection acceptance. Repository/gh
/// lookup belongs to the portable CLI tests; this fixture never fabricates a
/// snapshot, bypasses the metadata owner or adds a production bridge seam.
enum WorkspacePullRequestSmoke {
    static func set(workspace: UUID, number: UInt64, url: String) async throws {
        guard let path = ProcessInfo.processInfo.environment["ZMUX_SOCKET"], path.hasPrefix("/") else {
            throw BridgeFailure(message: "PR projection smoke requires the exact owned app socket")
        }
        let request = try JSONSerialization.data(withJSONObject: [
            "version": 2, "request_id": 1,
            "command": ["method": "targeted", "request": [
                "scope": ["workspace": workspace.uuidString],
                "operation": ["operation": "metadata", "action": [
                    "action": "set_pull_request", "pull_request": ["number": NSNumber(value: number), "url": url]
                ]]
            ]]
        ])
        let response = try await Task.detached { try exchange(path: path, request: request) }.value
        guard let reply = try JSONSerialization.jsonObject(with: response) as? [String: Any],
              reply["ok"] as? Bool == true else {
            throw BridgeFailure(message: "Shared socket refused native PR projection fixture")
        }
    }

    private static func exchange(path: String, request: Data) throws -> Data {
        func failure(_ operation: String) -> BridgeFailure {
            BridgeFailure(message: "Native PR socket \(operation) failed (errno \(errno))")
        }
        let descriptor = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw failure("create") }
        defer { Darwin.close(descriptor) }
        guard fcntl(descriptor, F_SETFL, O_NONBLOCK) == 0,
              fcntl(descriptor, F_SETFD, FD_CLOEXEC) == 0 else { throw failure("configure") }
        var enabled: Int32 = 1
        guard setsockopt(descriptor, SOL_SOCKET, SO_NOSIGPIPE, &enabled, socklen_t(MemoryLayout.size(ofValue: enabled))) == 0 else {
            throw failure("suppress SIGPIPE")
        }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        let encoded = Array(path.utf8) + [0]
        guard encoded.count <= MemoryLayout.size(ofValue: address.sun_path), request.count <= 16_384 else {
            throw BridgeFailure(message: "Native PR socket fixture exceeds path/request bound")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: encoded) }
        let deadline = ContinuousClock.now + .seconds(2)
        func ready(_ events: Int16) throws {
            while true {
                let left = ContinuousClock.now.duration(to: deadline)
                guard left > .zero else { throw BridgeFailure(message: "Native PR socket fixture deadline") }
                let parts = left.components
                let milliseconds = max(1, min(2000, parts.seconds * 1000 + parts.attoseconds / 1_000_000_000_000_000))
                var item = pollfd(fd: descriptor, events: events, revents: 0)
                let count = Darwin.poll(&item, 1, Int32(milliseconds))
                if count < 0, errno == EINTR { continue }
                if count == 0 { throw BridgeFailure(message: "Native PR socket fixture deadline") }
                guard count > 0 else { throw failure("poll") }
                guard item.revents & Int16(POLLNVAL) == 0 else { throw failure("descriptor") }
                if item.revents & (events | Int16(POLLHUP) | Int16(POLLERR)) != 0 { return }
            }
        }
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if connected != 0 {
            guard errno == EINPROGRESS || errno == EAGAIN else { throw failure("connect") }
            try ready(Int16(POLLOUT))
            var error: Int32 = 0
            var size = socklen_t(MemoryLayout.size(ofValue: error))
            guard getsockopt(descriptor, SOL_SOCKET, SO_ERROR, &error, &size) == 0 else { throw failure("connection status") }
            guard error == 0 else { throw BridgeFailure(message: "Native PR socket connection failed (SO_ERROR \(error))") }
        }
        var length = UInt32(request.count).bigEndian
        var frame = withUnsafeBytes(of: &length) { Data($0) }
        frame.append(request)
        try frame.withUnsafeBytes { storage in
            var offset = 0
            while offset < storage.count {
                try ready(Int16(POLLOUT))
                let count = Darwin.send(descriptor, storage.baseAddress!.advanced(by: offset), storage.count - offset, 0)
                if count < 0, errno == EAGAIN || errno == EINTR { continue }
                guard count > 0 else { throw failure("write") }
                offset += count
            }
        }
        func read(_ count: Int) throws -> Data {
            var data = Data(count: count)
            try data.withUnsafeMutableBytes { storage in
                var offset = 0
                while offset < storage.count {
                    try ready(Int16(POLLIN))
                    let received = Darwin.recv(descriptor, storage.baseAddress!.advanced(by: offset), storage.count - offset, 0)
                    if received < 0, errno == EAGAIN || errno == EINTR { continue }
                    guard received > 0 else { throw failure("read/EOF") }
                    offset += received
                }
            }
            return data
        }
        let responseBytes = try read(4).reduce(0) { ($0 << 8) | Int($1) }
        guard (1...1_048_576).contains(responseBytes) else { throw BridgeFailure(message: "Native PR socket response exceeds bound") }
        return try read(responseBytes)
    }
}
