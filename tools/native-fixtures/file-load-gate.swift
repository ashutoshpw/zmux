import Foundation

/// Runs the production load gate (`FileLoadScheduler`, `FileLoadPolicy`, `FileLoadGate`) against
/// scripted file requests. There are no threads, timers or sleeps: every request waits on an explicit
/// latch the fixture opens, the back-off pause is a scripted function, and "wait until the gate has
/// settled" is a bounded loop of `Task.yield()` on the main actor, so every run is the same run.

struct ScriptedFault: FileLoadFailure {
    let code: String
    var outcomeUnknown = false
}

typealias ScriptedResult = Result<String, ScriptedFault>

@MainActor
final class Flag {
    var value = false
}

/// The file service as the fixture scripts it: each request id has a list of results, one per
/// attempt, and anything beyond the list succeeds. With `holdAll` a request does not finish until
/// `releaseOne` opens the oldest latch.
@MainActor
final class ScriptedService {
    var results: [Int: [ScriptedResult]] = [:]
    var holdAll = false
    private(set) var calls: [Int] = []
    private(set) var active = 0
    private(set) var peakActive = 0
    private var latches: [CheckedContinuation<Void, Never>] = []
    private var attempts: [Int: Int] = [:]

    var heldCount: Int { latches.count }

    func perform(_ id: Int) async -> ScriptedResult {
        calls.append(id)
        active += 1
        peakActive = max(peakActive, active)
        if holdAll { await withCheckedContinuation { latches.append($0) } }
        active -= 1
        let attempt = attempts[id, default: 0]
        attempts[id] = attempt + 1
        let scripted = results[id] ?? []
        return attempt < scripted.count ? scripted[attempt] : .success("ok-" + String(id))
    }

    func releaseOne() {
        precondition(!latches.isEmpty, "nothing is held")
        latches.removeFirst().resume()
    }
}

/// The back-off `pause` the gate is given. It records every requested duration and returns at
/// once, unless `holdNext` asks it to wait until `resume` is called.
@MainActor
final class ScriptedPauses {
    private(set) var requested: [Int] = []
    var holdNext = false
    private var latch: CheckedContinuation<Void, Never>?

    var isHeld: Bool { latch != nil }

    func pause(_ milliseconds: Int) async {
        requested.append(milliseconds)
        if holdNext {
            holdNext = false
            await withCheckedContinuation { latch = $0 }
        }
    }

    func resume() {
        precondition(latch != nil, "no pause is held")
        latch?.resume()
        latch = nil
    }
}

@MainActor
final class ProgressLog {
    private(set) var events: [Int: [FileLoadProgress]] = [:]
    /// The order in which requests first reported anything: the order they asked the gate for a slot.
    private(set) var entryOrder: [Int] = []

    func add(_ id: Int, _ progress: FileLoadProgress) {
        if events[id] == nil { entryOrder.append(id) }
        events[id, default: []].append(progress)
    }
}

@main
@MainActor
struct FileLoadGateProbe {
    static var failures: [String] = []

    static func expect(_ condition: Bool, _ message: String) {
        if !condition { failures.append(message) }
    }

    /// Lets every other main-actor task run until `condition` holds. Bounded, never time based.
    static func settle(_ what: String, _ condition: @MainActor () -> Bool) async {
        var spins = 0
        while !condition() && spins < 200_000 {
            await Task.yield()
            spins += 1
        }
        expect(condition(), "never settled: " + what)
    }

    static func launch(_ id: Int, gate: FileLoadGate, service: ScriptedService, log: ProgressLog,
                       cancelled: Flag? = nil) -> Task<ScriptedResult?, Never> {
        Task { @MainActor in
            await gate.run(isCancelled: { cancelled?.value ?? false },
                           progress: { log.add(id, $0) },
                           operation: { await service.perform(id) })
        }
    }

    // MARK: - The pure queue

    static func scheduler() {
        var queue = FileLoadScheduler(limit: 3)
        let admissions = (1...16).map { queue.request(UInt64($0)) }
        expect(admissions[0] == .start && admissions[1] == .start && admissions[2] == .start, "the first three requests start")
        for index in 3..<16 {
            expect(admissions[index] == .wait(position: index - 2), "request \(index + 1) waits at position \(index - 2), got \(admissions[index])")
        }
        expect(queue.runningCount == 3 && queue.waitingCount == 13, "sixteen requests: three running and thirteen waiting")
        expect(queue.request(1) == .start && queue.request(5) == .wait(position: 2) && queue.waitingCount == 13,
               "asking again for a known ticket changes nothing")
        expect(queue.position(of: 4) == 1 && queue.position(of: 16) == 13 && queue.position(of: 1) == nil, "positions count from the next to start")

        expect(queue.finish(99).isEmpty && queue.runningCount == 3, "finishing an unknown ticket changes nothing")
        expect(queue.finish(2) == [4], "freeing a slot starts the oldest waiting ticket")
        expect(queue.position(of: 5) == 1, "the queue closes up behind it")
        expect(queue.finish(16).isEmpty && queue.waitingCount == 11, "an abandoned waiting ticket leaves without starting anything")
        expect(queue.finish(16).isEmpty, "finishing it twice changes nothing")

        // Complete in an order unrelated to the start order: never more than three run, FIFO start.
        var started: [UInt64] = []
        var peak = queue.runningCount
        while let next = queue.running.first {
            started.append(contentsOf: queue.finish(next))
            peak = max(peak, queue.runningCount)
        }
        expect(peak <= 3, "never more than the limit run at once, saw \(peak)")
        expect(started == Array(5...15).map { UInt64($0) }, "waiting tickets start oldest first: \(started)")
        expect(queue.runningCount == 0 && queue.waitingCount == 0, "everything drained")

        var single = FileLoadScheduler(limit: 1)
        expect(single.request(1) == .start && single.request(2) == .wait(position: 1) && single.finish(1) == [2], "a limit of one runs one at a time")
        expect(single.finish(2).isEmpty && single.runningCount == 0, "and ends empty")
    }

    // MARK: - The retry rule

    static func decisions() {
        let policy = FileLoadPolicy.standard
        expect(policy.maxInFlight == 3 && policy.maxAttempts == 4 && policy.backoffMilliseconds == [250, 500, 1_000],
               "the production policy is three in flight, four tries, 250/500/1000 ms")
        expect(policy.backoffMilliseconds.count == policy.maxAttempts - 1, "there is one back-off before each retry")
        expect(policy.transientCodes == ["busy", "file_worker_unavailable", "timeout"], "exactly the busy-like codes are transient")
        for code in ["busy", "file_worker_unavailable", "timeout"] {
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 1) == .retry(afterMilliseconds: 250), "\(code) retries after 250 ms")
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 2) == .retry(afterMilliseconds: 500), "\(code) retries after 500 ms")
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 3) == .retry(afterMilliseconds: 1_000), "\(code) retries after 1000 ms")
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 4) == .giveUp, "\(code) gives up after the fourth try")
            expect(policy.decision(code: code, outcomeUnknown: true, attempt: 1) == .giveUp, "\(code) with an unknown outcome is never retried")
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 0) == .giveUp, "attempt 0 is not a valid count of tries")
        }
        for code in ["not_found", "permission_denied", "too_large", "binary_file", "invalid_utf8", "is_directory", "not_regular_file",
                     "path_changed", "content_mismatch", "stale_owner", "unknown_surface", "host_conflict", "closed", "invalid_reply",
                     "invalid_request", "conflict", "transport", "internal"] {
            expect(policy.decision(code: code, outcomeUnknown: false, attempt: 1) == .giveUp, "\(code) is not transient and must not be retried")
        }
    }

    static func bridgeRefusals() {
        let policy = FileLoadPolicy.standard
        for status: Int32 in [-4, -5] {
            let refusal = FileBridgeRefusal(status: status)
            expect(refusal.code == "busy" && !refusal.outcomeUnknown, "bridge status \(status) is refused before anything ran, so it is the transient busy code")
            expect(policy.decision(code: refusal.code, outcomeUnknown: refusal.outcomeUnknown, attempt: 1) == .retry(afterMilliseconds: 250),
                   "bridge status \(status) is retried")
        }
        let malformed = FileBridgeRefusal(status: -2)
        expect(malformed.code == "transport" && !malformed.outcomeUnknown, "a malformed frame is a transport failure that ran nothing")
        expect(policy.decision(code: malformed.code, outcomeUnknown: malformed.outcomeUnknown, attempt: 1) == .giveUp, "a malformed frame is not retried")
        for status: Int32 in [-1, -3, -6, 1, 99] {
            let refusal = FileBridgeRefusal(status: status)
            expect(refusal.code == "transport" && refusal.outcomeUnknown, "bridge status \(status) leaves the effect unknown")
            expect(policy.decision(code: refusal.code, outcomeUnknown: refusal.outcomeUnknown, attempt: 1) == .giveUp, "bridge status \(status) is never retried")
        }
    }

    // MARK: - Sixteen tabs at once

    static func sixteenAtOnce() async {
        let service = ScriptedService()
        service.holdAll = true
        let pauses = ScriptedPauses()
        let log = ProgressLog()
        let gate = FileLoadGate(pause: { milliseconds in await pauses.pause(milliseconds) })
        let tasks = (0..<16).map { launch($0, gate: gate, service: service, log: log) }

        await settle("three requests running and thirteen waiting") { service.heldCount == 3 && gate.waitingCount == 13 }
        expect(gate.inFlightCount == 3, "exactly three are in flight, not sixteen")
        expect(service.calls.count == 3, "only three requests reached the file service, saw \(service.calls.count)")
        let waiting = log.entryOrder.compactMap { id -> Int? in
            if case .waiting(let position)? = log.events[id]?.first { return position }
            return nil
        }
        expect(waiting.sorted() == Array(1...13), "thirteen tabs show a waiting state with distinct positions 1...13: \(waiting.sorted())")

        var released = 0
        while released < 16 {
            await settle("a request is held", { service.heldCount > 0 })
            service.releaseOne()
            released += 1
            await settle("the freed slot is taken", { service.heldCount == min(3, 16 - released) })
            expect(service.peakActive <= 3 && gate.inFlightCount <= 3, "no more than three requests are ever in flight")
        }
        var successes = 0
        for (id, task) in tasks.enumerated() {
            if case .success(let value)? = await task.value, value == "ok-" + String(id) { successes += 1 }
        }
        expect(successes == 16, "all sixteen loads succeed with their own reply, got \(successes)")
        expect(service.peakActive == 3 && gate.peakInFlightCount == 3, "the limit of three was reached and never exceeded (service \(service.peakActive), gate \(gate.peakInFlightCount))")
        expect(gate.peakWaitingCount == 13, "thirteen tabs queued at the peak, saw \(gate.peakWaitingCount)")
        expect(service.calls == log.entryOrder, "requests start in the order they asked: \(service.calls) vs \(log.entryOrder)")
        expect(gate.inFlightCount == 0 && gate.waitingCount == 0, "the gate is empty afterwards")
        expect(pauses.requested.isEmpty, "no back-off was needed when nothing failed")
    }

    // MARK: - Retry

    static func busyRetries() async {
        let busy: ScriptedResult = .failure(ScriptedFault(code: "busy"))
        let service = ScriptedService()
        service.results[0] = [busy, busy]
        let pauses = ScriptedPauses()
        let log = ProgressLog()
        let gate = FileLoadGate(pause: { milliseconds in await pauses.pause(milliseconds) })
        let result = await launch(0, gate: gate, service: service, log: log).value
        if case .success(let value)? = result { expect(value == "ok-0", "the retried request returns its reply") }
        else { expect(false, "busy twice then success must end in success, got \(String(describing: result))") }
        expect(service.calls == [0, 0, 0], "busy triggers a retry each time: three tries, saw \(service.calls)")
        expect(pauses.requested == [250, 500], "back-off grows 250 then 500 ms, saw \(pauses.requested)")
        let events = log.events[0] ?? []
        expect(events.contains(.retrying(attempt: 1, of: 4, afterMilliseconds: 250, code: "busy"))
               && events.contains(.retrying(attempt: 2, of: 4, afterMilliseconds: 500, code: "busy")),
               "the panel is told it is retrying: \(events)")
        expect(events.contains(.running(attempt: 3)), "and that the third try is running: \(events)")
        expect(gate.inFlightCount == 0, "no slot is left held")

        // Every transient code retries.
        for code in ["file_worker_unavailable", "timeout"] {
            let one = ScriptedService()
            one.results[0] = [.failure(ScriptedFault(code: code))]
            let again = ScriptedPauses()
            let g = FileLoadGate(pause: { milliseconds in await again.pause(milliseconds) })
            let outcome = await launch(0, gate: g, service: one, log: ProgressLog()).value
            if case .success? = outcome, one.calls == [0, 0], again.requested == [250] {} else { expect(false, "\(code) must be retried once and then succeed") }
        }
    }

    static func exhaustion() async {
        let service = ScriptedService()
        service.results[0] = Array(repeating: .failure(ScriptedFault(code: "busy")), count: 10)
        let pauses = ScriptedPauses()
        let gate = FileLoadGate(pause: { milliseconds in await pauses.pause(milliseconds) })
        let result = await launch(0, gate: gate, service: service, log: ProgressLog()).value
        if case .failure(let fault)? = result { expect(fault.code == "busy", "the explicit error is the busy failure") }
        else { expect(false, "a service that stays busy must end in the failure, got \(String(describing: result))") }
        expect(service.calls == [0, 0, 0, 0], "exactly four tries, saw \(service.calls)")
        expect(pauses.requested == [250, 500, 1_000], "three back-offs, saw \(pauses.requested)")
        expect(gate.inFlightCount == 0 && gate.waitingCount == 0, "nothing is left held")
    }

    static func nonTransientFailuresAreNotRetried() async {
        for fault in [ScriptedFault(code: "not_found"), ScriptedFault(code: "stale_owner"), ScriptedFault(code: "transport"),
                      ScriptedFault(code: "busy", outcomeUnknown: true), ScriptedFault(code: "timeout", outcomeUnknown: true)] {
            let service = ScriptedService()
            service.results[0] = [.failure(fault), .success("never")]
            let pauses = ScriptedPauses()
            let gate = FileLoadGate(pause: { milliseconds in await pauses.pause(milliseconds) })
            let result = await launch(0, gate: gate, service: service, log: ProgressLog()).value
            if case .failure(let returned)? = result { expect(returned.code == fault.code, "the original failure is returned") }
            else { expect(false, "\(fault.code) (outcome unknown: \(fault.outcomeUnknown)) must fail at once, got \(String(describing: result))") }
            expect(service.calls == [0], "\(fault.code) (outcome unknown: \(fault.outcomeUnknown)) is tried once, saw \(service.calls)")
            expect(pauses.requested.isEmpty, "and never waits")
            expect(gate.inFlightCount == 0, "and releases its slot")
        }
    }

    static func backoffHoldsNoSlot() async {
        let service = ScriptedService()
        service.results[0] = [.failure(ScriptedFault(code: "busy"))]
        let pauses = ScriptedPauses()
        pauses.holdNext = true
        let one = FileLoadPolicy(maxInFlight: 1, maxAttempts: 4, backoffMilliseconds: [250, 500, 1_000], transientCodes: FileLoadPolicy.defaultTransientCodes)
        let gate = FileLoadGate(policy: one, pause: { milliseconds in await pauses.pause(milliseconds) })
        let log = ProgressLog()
        let first = launch(0, gate: gate, service: service, log: log)
        await settle("the first request is backing off") { pauses.isHeld }
        expect(gate.inFlightCount == 0, "a request that is backing off holds no slot")
        let second = launch(1, gate: gate, service: service, log: log)
        await settle("the second request ran during the first one's back-off") { service.calls == [0, 1] }
        pauses.resume()
        if case .success? = await second.value {} else { expect(false, "the second request should succeed") }
        if case .success? = await first.value {} else { expect(false, "the first request should succeed on its retry") }
        expect(service.calls == [0, 1, 0], "the retry went to the back of the line: \(service.calls)")
        expect(gate.inFlightCount == 0 && gate.waitingCount == 0, "nothing is left held")
    }

    static func cancelledRequestsNeverReachTheService() async {
        // Closed while waiting for a slot.
        let service = ScriptedService()
        service.holdAll = true
        let gate = FileLoadGate(policy: FileLoadPolicy(maxInFlight: 1, maxAttempts: 4, backoffMilliseconds: [250, 500, 1_000],
                                                       transientCodes: FileLoadPolicy.defaultTransientCodes),
                                pause: { _ in })
        let flag = Flag()
        let log = ProgressLog()
        let first = launch(0, gate: gate, service: service, log: log)
        await settle("the first request is held") { service.heldCount == 1 }
        let second = launch(1, gate: gate, service: service, log: log, cancelled: flag)
        let third = launch(2, gate: gate, service: service, log: log)
        await settle("two requests are waiting") { gate.waitingCount == 2 }
        flag.value = true
        service.releaseOne()
        await settle("the third request took the slot the cancelled one passed on") { service.calls == [0, 2] && service.heldCount == 1 }
        service.releaseOne()
        expect(await first.value != nil, "the running request completes")
        expect(await second.value == nil, "a request cancelled while waiting returns nil")
        if case .success? = await third.value {} else { expect(false, "the request behind a cancelled one still runs") }
        expect(service.calls == [0, 2], "a cancelled request never reaches the file service: \(service.calls)")
        expect(gate.inFlightCount == 0 && gate.waitingCount == 0, "the cancelled request leaves nothing behind")

        // Closed while backing off.
        let retrying = ScriptedService()
        retrying.results[0] = [.failure(ScriptedFault(code: "busy"))]
        let pauses = ScriptedPauses()
        pauses.holdNext = true
        let backoffGate = FileLoadGate(pause: { milliseconds in await pauses.pause(milliseconds) })
        let closed = Flag()
        let task = launch(0, gate: backoffGate, service: retrying, log: ProgressLog(), cancelled: closed)
        await settle("the request is backing off") { pauses.isHeld }
        closed.value = true
        pauses.resume()
        expect(await task.value == nil, "a request cancelled during its back-off returns nil")
        expect(retrying.calls == [0], "and does not try again: \(retrying.calls)")
    }

    // MARK: - What the panel shows

    static func visibleStates() {
        expect(FileLoadProgress.waiting(position: 2).loadingTitle(for: "a.txt") == "Waiting to open a.txt… (2 in line)", "a queued tab says it is waiting and where")
        expect(FileLoadProgress.running(attempt: 1).loadingTitle(for: "a.txt") == "Opening a.txt…", "a running tab says it is opening")
        expect(FileLoadProgress.retrying(attempt: 1, of: 4, afterMilliseconds: 250, code: "busy").loadingTitle(for: "a.txt").contains("busy"),
               "a retrying tab says why")
        expect(FileLoadProgress.waiting(position: 1).statusNote?.contains("Waiting") == true, "the header notes a wait")
        expect(FileLoadProgress.running(attempt: 2).statusNote == nil, "a running load adds no header note")
        expect(FileLoadProgress.retrying(attempt: 2, of: 4, afterMilliseconds: 500, code: "busy").statusNote?.contains("try 3 of 4") == true,
               "the header counts the next try")
    }

    static func main() async {
        scheduler()
        decisions()
        bridgeRefusals()
        visibleStates()
        await sixteenAtOnce()
        await busyRetries()
        await exhaustion()
        await nonTransientFailuresAreNotRetried()
        await backoffHoldsNoSlot()
        await cancelledRequestsNeverReachTheService()
        guard failures.isEmpty else {
            for failure in failures { FileHandle.standardError.write(Data(("file load gate fixture failed: " + failure + "\n").utf8)) }
            exit(1)
        }
        print("File load gate passed: sixteen simultaneous loads never exceed three in flight and start in order, waiting tabs report their position, busy-like failures retry with 250/500/1000 ms back-off and then fail explicitly, other failures and unknown outcomes are never retried, a backing-off request holds no slot, cancelled requests never reach the service.")
    }
}
