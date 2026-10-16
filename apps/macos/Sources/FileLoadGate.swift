import Foundation

// Admission control and bounded retry for the file requests that load a whole file (attach and
// reload). Every file tab sends one when its snapshot first arrives, so opening or restoring many
// tabs at once would otherwise start that many bridge requests together. The Rust file worker
// admits a fixed number of queued jobs (`QUEUE_BOUND`, 8, plus the one running) and answers `busy`
// at once when full, so the overload would be created here and every tab past the limit would land
// in the error state. The scheduling decisions below are pure and Foundation-only so that
// `scripts/check-file-load-gate` can run the real code on macOS; the Rust contract test
// `the_swift_load_gate_keeps_sixteen_tabs_inside_what_the_rust_worker_admits` ties the numbers to
// the worker's real bound.

/// How a nonzero status of the bridge export `zmux_desktop_file` is reported. `ZMUX_BUSY` (-4: the
/// bridge registry was contended) and `ZMUX_CAPACITY` (-5: every one of the 16 reply buffers was
/// reserved) are returned before the request reaches the file service, so nothing ran and the
/// request is safe to send again; they are the bridge-level form of the worker's `busy` and are
/// reported as that code. `ZMUX_INVALID_ARGUMENT` (-2) is a frame Rust refused. Any other status
/// leaves the effect unknown.
struct FileBridgeRefusal: Equatable, Sendable {
    let code: String
    let message: String
    let outcomeUnknown: Bool

    static let busyStatus: Int32 = -4
    static let capacityStatus: Int32 = -5
    static let invalidArgumentStatus: Int32 = -2

    init(status: Int32) {
        switch status {
        case Self.busyStatus:
            self.init(code: "busy", message: "The file channel was busy; nothing was started", outcomeUnknown: false)
        case Self.capacityStatus:
            self.init(code: "busy", message: "The file channel had no free reply buffer; nothing was started", outcomeUnknown: false)
        case Self.invalidArgumentStatus:
            self.init(code: "transport", message: "The file channel refused a malformed request (status -2); nothing was started", outcomeUnknown: false)
        default:
            self.init(code: "transport", message: "The file channel returned status \(status); the operation was not replayed", outcomeUnknown: true)
        }
    }

    private init(code: String, message: String, outcomeUnknown: Bool) {
        self.code = code
        self.message = message
        self.outcomeUnknown = outcomeUnknown
    }
}

/// What the gate needs to know about a failed request. `FileHostFailure` is the production
/// conformer; the macOS fixture supplies its own.
protocol FileLoadFailure: Error {
    var code: String { get }
    /// True when the request may have taken effect although it reported a failure.
    var outcomeUnknown: Bool { get }
}

/// The numbers and the retry rule. The values are `static let`s so a Rust contract test can read
/// them from this file and compare them with the file worker's real queue bound.
struct FileLoadPolicy: Sendable, Equatable {
    /// Whole-file requests allowed to be in flight at once across the application. The Rust
    /// worker queues 8 jobs and runs 1; saves, observations and `File > Open` probes share that
    /// queue, so a load may use at most about a third of it.
    static let defaultMaxInFlight = 3
    /// Tries per request, including the first. After the last one the failure is shown with Retry.
    static let defaultMaxAttempts = 4
    /// Pause before the second, third and fourth try. One entry fewer than `maxAttempts`.
    static let defaultBackoffMilliseconds = [250, 500, 1_000]
    /// Failures that mean "the file service could not take this right now". Nothing else is retried.
    static let defaultTransientCodes: Set<String> = ["busy", "file_worker_unavailable", "timeout"]

    static let standard = FileLoadPolicy(maxInFlight: defaultMaxInFlight, maxAttempts: defaultMaxAttempts,
                                         backoffMilliseconds: defaultBackoffMilliseconds, transientCodes: defaultTransientCodes)

    let maxInFlight: Int
    let maxAttempts: Int
    let backoffMilliseconds: [Int]
    let transientCodes: Set<String>

    /// `attempt` is the number of tries already made (1 after the first).
    func decision(code: String, outcomeUnknown: Bool, attempt: Int) -> FileLoadDecision {
        guard transientCodes.contains(code), !outcomeUnknown, attempt >= 1, attempt < maxAttempts,
              attempt <= backoffMilliseconds.count else { return .giveUp }
        return .retry(afterMilliseconds: backoffMilliseconds[attempt - 1])
    }
}

enum FileLoadDecision: Equatable, Sendable {
    case retry(afterMilliseconds: Int)
    case giveUp
}

/// What a load is doing, for the panel to show. `waiting` and `retrying` are the visible states
/// that replace a silent stall.
enum FileLoadProgress: Equatable, Sendable {
    /// Queued behind other loads; `position` is 1 for the next one to start.
    case waiting(position: Int)
    case running(attempt: Int)
    case retrying(attempt: Int, of: Int, afterMilliseconds: Int, code: String)

    /// The loading-screen headline for a panel that has nothing to show yet.
    func loadingTitle(for name: String) -> String {
        switch self {
        case .waiting(let position):
            return "Waiting to open " + name + "… (" + String(position) + " in line)"
        case .running:
            return "Opening " + name + "…"
        case .retrying(let attempt, let total, _, _):
            return "The file service is busy. Opening " + name + " again… (try " + String(attempt + 1) + " of " + String(total) + ")"
        }
    }

    /// A short note for the header; nil when the load is simply running.
    var statusNote: String? {
        switch self {
        case .waiting(let position): return "Waiting for the file service (" + String(position) + " in line)"
        case .running: return nil
        case .retrying(let attempt, let total, _, _): return "File service busy, retrying (try " + String(attempt + 1) + " of " + String(total) + ")"
        }
    }
}

/// The FIFO of loads waiting for a slot. A value type with no clock, task or I/O: tickets go in,
/// the tickets that may start come out. Never more than `limit` tickets run.
struct FileLoadScheduler: Sendable, Equatable {
    enum Admission: Equatable, Sendable {
        case start
        case wait(position: Int)
    }

    let limit: Int
    private(set) var running: [UInt64] = []
    private(set) var queued: [UInt64] = []

    init(limit: Int) {
        precondition(limit >= 1, "a load gate needs at least one slot")
        self.limit = limit
    }

    var runningCount: Int { running.count }
    var waitingCount: Int { queued.count }

    /// Asks for a slot. Asking again for a known ticket reports where it is.
    mutating func request(_ ticket: UInt64) -> Admission {
        if running.contains(ticket) { return .start }
        if let index = queued.firstIndex(of: ticket) { return .wait(position: index + 1) }
        if running.count < limit && queued.isEmpty {
            running.append(ticket)
            return .start
        }
        queued.append(ticket)
        return .wait(position: queued.count)
    }

    /// The ticket's request is over (success, failure or abandoned). Returns the tickets that now
    /// start, oldest first. An unknown ticket changes nothing.
    mutating func finish(_ ticket: UInt64) -> [UInt64] {
        if let index = queued.firstIndex(of: ticket) {
            queued.remove(at: index)
            return []
        }
        guard let index = running.firstIndex(of: ticket) else { return [] }
        running.remove(at: index)
        var started: [UInt64] = []
        while running.count < limit, !queued.isEmpty {
            let next = queued.removeFirst()
            running.append(next)
            started.append(next)
        }
        return started
    }

    /// Where a waiting ticket stands now (1 is next), or nil when it is running or unknown.
    func position(of ticket: UInt64) -> Int? {
        queued.firstIndex(of: ticket).map { $0 + 1 }
    }
}

/// Runs whole-file requests through a `FileLoadScheduler` and retries the ones the file service
/// could not take. Everything happens on the main actor, so there are no locks: a slot is handed to
/// the next waiter in the same step that frees it, and a request that is backing off holds no slot.
@MainActor
final class FileLoadGate {
    private var scheduler: FileLoadScheduler
    private var admissions: [UInt64: CheckedContinuation<Void, Never>] = [:]
    private var nextTicket: UInt64 = 1
    let policy: FileLoadPolicy
    private let pause: @Sendable (Int) async -> Void

    /// `pause` waits the given milliseconds; the default sleeps, the fixture passes a scripted one.
    init(policy: FileLoadPolicy = .standard,
         pause: @escaping @Sendable (Int) async -> Void = { milliseconds in
             // A cancelled sleep returns early; `run` then makes its next try sooner. That is
             // bounded by `maxAttempts` and never skips a failure report.
             _ = try? await Task.sleep(for: .milliseconds(milliseconds))
         }) {
        self.policy = policy
        self.pause = pause
        scheduler = FileLoadScheduler(limit: policy.maxInFlight)
    }

    var inFlightCount: Int { scheduler.runningCount }
    var waitingCount: Int { scheduler.waitingCount }
    /// The most requests that were ever in flight, and the most that ever waited, since the gate was
    /// made. Evidence for tests and the in-app smoke that the limit held.
    private(set) var peakInFlightCount = 0
    private(set) var peakWaitingCount = 0

    /// Runs `operation` once a slot is free, and again after a back-off while it fails with a
    /// transient code, up to `policy.maxAttempts` times. Returns the last result, or nil when
    /// `isCancelled` became true before a request was sent (the operation then never runs).
    /// A non-transient failure, or one whose outcome is unknown, is returned at once.
    func run<Value, Fault: FileLoadFailure>(
        isCancelled: @MainActor () -> Bool,
        progress: @MainActor (FileLoadProgress) -> Void,
        operation: @MainActor () async -> Result<Value, Fault>
    ) async -> Result<Value, Fault>? {
        var attempt = 1
        while true {
            let ticket = nextTicket
            nextTicket += 1
            switch scheduler.request(ticket) {
            case .start:
                break
            case .wait(let position):
                peakWaitingCount = max(peakWaitingCount, scheduler.waitingCount)
                progress(.waiting(position: position))
                await withCheckedContinuation { admissions[ticket] = $0 }
            }
            peakInFlightCount = max(peakInFlightCount, scheduler.runningCount)
            if isCancelled() {
                release(ticket)
                return nil
            }
            progress(.running(attempt: attempt))
            let result = await operation()
            release(ticket)
            guard case .failure(let fault) = result else { return result }
            switch policy.decision(code: fault.code, outcomeUnknown: fault.outcomeUnknown, attempt: attempt) {
            case .giveUp:
                return result
            case .retry(let milliseconds):
                progress(.retrying(attempt: attempt, of: policy.maxAttempts, afterMilliseconds: milliseconds, code: fault.code))
                await pause(milliseconds)
                if isCancelled() { return nil }
                attempt += 1
            }
        }
    }

    private func release(_ ticket: UInt64) {
        for next in scheduler.finish(ticket) {
            guard let waiter = admissions.removeValue(forKey: next) else {
                preconditionFailure("a queued load started without a waiter to resume")
            }
            waiter.resume()
        }
    }
}
