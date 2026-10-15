import Darwin
import Foundation

enum NativeStoragePathFailure: Error, LocalizedError {
    case invalidPath
    case system(path: String, code: Int32)

    var errorDescription: String? {
        switch self {
        case .invalidPath: return "Storage directory must be an absolute path without traversal or NUL."
        case .system(let path, let code): return "Cannot resolve storage directory \(path) (errno \(code))."
        }
    }
}

/// Keep the physical spelling required by Rust's component-by-component nofollow walk.
/// Foundation's symlink resolver can shorten an existing /private path back to an alias.
enum NativeStoragePath {
    static func resolve(_ path: String) throws -> String {
        let components = path.split(separator: "/")
        guard path.hasPrefix("/"), !path.utf8.contains(0),
              !components.contains(where: { $0 == "." || $0 == ".." }) else {
            throw NativeStoragePathFailure.invalidPath
        }
        var remaining = components.map(String.init)
        var missing: [String] = []
        var candidate = path
        while true {
            let (resolved, code) = candidate.withCString { value in
                let resolved = Darwin.realpath(value, nil)
                return (resolved, errno)
            }
            if let resolved {
                defer { Darwin.free(resolved) }
                guard let physical = String(validatingCString: resolved) else {
                    throw NativeStoragePathFailure.invalidPath
                }
                let directory = Darwin.open(physical, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                guard directory >= 0 else {
                    throw NativeStoragePathFailure.system(path: physical, code: errno)
                }
                guard Darwin.close(directory) == 0 else {
                    throw NativeStoragePathFailure.system(path: physical, code: errno)
                }
                let suffix = missing.reversed().joined(separator: "/")
                return suffix.isEmpty ? physical : (physical == "/" ? "/" : physical + "/") + suffix
            }
            // A missing first-launch directory is expected. Permission, non-directory
            // and symlink-loop errors must never select a different ancestor.
            guard code == ENOENT, let component = remaining.popLast() else {
                throw NativeStoragePathFailure.system(path: candidate, code: code)
            }
            missing.append(component)
            candidate = "/" + remaining.joined(separator: "/")
        }
    }
}
