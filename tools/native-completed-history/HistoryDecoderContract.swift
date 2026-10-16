import Foundation

private struct HistoryContractFailure: Error { let message: String }

/// Decode independent history wire through the actual app Models dependency
/// closure. These historical fixture values provide no live authentication,
/// Run, workload startup or native presentation proof.
@main
enum HistoryDecoderContract {
    private typealias Object = [String: Any]

    private static let rawV1 = #"""
{"version":1,"domain":"zmux.local-zellij.terminal.v1","binding":{"version":1,"owner_uuid":"00000000-0000-0000-0000-000000000014","owner_generation":"00000000-0000-0000-0000-000000000015","namespace":{"root":[1,11],"socket":[1,12]},"completion_socket":[1,13],"server":{"pid":101,"uid":1000,"parent_pid":1,"start":[20,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zellij"},"supervisor":{"pid":102,"uid":1000,"parent_pid":101,"start":[30,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"workload":{"pid":103,"uid":1000,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/bin/sh"}},"binding_digest":"ada9261a1a1bd868e4b920118c71247a61ba3bdc5cabbea46737aad6ce06050e","terminal_sequence":1,"workload_exit":{"kind":"exited","value":7},"child_reaped":true}
"""#
    private static let admittedV2 = #"""
{"execution_contract":"admitted_lifetime_v2","receipt":{"version":2,"domain":"zmux.local-zellij.terminal.v2","binding":{"version":2,"domain":"zmux.local-zellij.execution.v2","owner_uuid":"00000000-0000-0000-0000-000000000014","owner_generation":"00000000-0000-0000-0000-000000000015","namespace":{"root":[1,11],"socket":[1,12]},"completion_socket":[1,13],"server":{"pid":101,"uid":1000,"parent_pid":1,"start":[20,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zellij"},"supervisor":{"pid":102,"uid":1000,"parent_pid":101,"start":[30,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"workload_birth":{"pid":103,"uid":1000,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"lifetime":{"pid":103,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111"},"invocation":{"shell":"/bin/sh","invocation_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"admission_id":"00000000-0000-0000-0000-000000000016"},"binding_digest":"20e3c6ee26ec6507888571e2af25149b82e71a0ba0ee38d4602667300acedfa1","terminal_sequence":1,"workload_exit":{"kind":"exited","value":7},"child_reaped":true,"execution_observation":{"kind":"not_observed"}}}
"""#

    private static func object(_ wire: String) throws -> Object {
        guard let value = try JSONSerialization.jsonObject(with: Data(wire.utf8)) as? Object else {
            throw HistoryContractFailure(message: "History fixture is not an object")
        }
        return value
    }

    private static func body(_ wire: Object, change: (inout Object) -> Void) -> Object {
        var result = wire
        if var nested = result["receipt"] as? Object {
            change(&nested)
            result["receipt"] = nested
        } else {
            change(&result)
        }
        return result
    }

    private static func decode(_ wire: Object) throws -> CompletedLocalZellijHistory.Receipt {
        let surface: Object = [
            "id": 3, "kind": "terminal", "session_id": NSNull(), "title": "Saved history",
            "navigation": 0,
            "local_zellij_completion": ["receipt": wire,
                "viewport": ["contents": "saved α界 e\u{0301}🙂  \n", "truncated": true]]
        ]
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let decoded = try decoder.decode(Surface.self, from: JSONSerialization.data(withJSONObject: surface))
        guard decoded.sessionId == nil, let history = decoded.localZellijCompletion,
              history.viewport?.contents == "saved α界 e\u{0301}🙂  \n", history.viewport?.truncated == true else {
            throw HistoryContractFailure(message: "Actual Surface lost sessionless bounded-history projection")
        }
        return history.receipt
    }

    private static func observation(_ receipt: CompletedLocalZellijHistory.Receipt) -> String {
        switch receipt.executionObservation {
        case nil: return "legacy"
        case .notObserved?: return "not_observed"
        case .shellObserved?: return "shell_observed"
        case .execFailed(let number)?: return "exec_failed:\(number)"
        }
    }

    static func main() throws {
        let legacy = try object(rawV1)
        let admitted = try object(admittedV2)
        let observed = body(admitted) { $0["execution_observation"] = ["kind": "shell_observed"] }
        let failed = body(admitted) {
            $0["workload_exit"] = ["kind": "exited", "value": 127]
            $0["execution_observation"] = ["kind": "exec_failed", "stage": "configured_shell_exec", "errno": 2]
        }
        let ordinary127 = body(admitted) { $0["workload_exit"] = ["kind": "exited", "value": 127] }
        let valid: [(String, Object, Int, String, Int, String, String?)] = [
            ("legacy_v1_raw", legacy, 1, "legacy", 7, "Process exited with status 7", nil),
            ("v2_not_observed", admitted, 2, "not_observed", 7, "Process exited with status 7", "Shell startup was not observed."),
            ("v2_shell_observed", observed, 2, "shell_observed", 7, "Process exited with status 7", nil),
            ("v2_exec_failed", failed, 2, "exec_failed:2", 127, "Shell launch failed (error 2)", "Process exited with status 127"),
            ("v2_ordinary_exit127", ordinary127, 2, "not_observed", 127, "Process exited with status 127", "Shell startup was not observed.")
        ]
        var accepted: [String] = []
        for (name, wire, version, expectedObservation, rawExit, status, detail) in valid {
            let receipt = try decode(wire)
            guard receipt.contractVersion == version, observation(receipt) == expectedObservation,
                  receipt.workloadExit.kind == .exited, receipt.workloadExit.value == rawExit,
                  receipt.statusLabel == status, receipt.executionDetail == detail,
                  receipt.bindingDigest.utf8.count == 64 else {
                throw HistoryContractFailure(message: "Actual Models classification differs: " + name)
            }
            accepted.append(name)
        }
        var acceptedBoundaries: [String] = []
        for (contract, wire) in [("v1", legacy), ("v2", admitted)] {
            for (kind, value) in [("exited", 0), ("exited", 255), ("signaled", 1), ("signaled", 127)] {
                let receipt = try decode(body(wire) { $0["workload_exit"] = ["kind": kind, "value": value] })
                guard receipt.workloadExit.kind.rawValue == kind, receipt.workloadExit.value == value else {
                    throw HistoryContractFailure(message: "Valid exit boundary was not preserved")
                }
                acceptedBoundaries.append("\(contract)_\(kind)_\(value)")
            }
        }

        var wrongContract = admitted
        wrongContract["execution_contract"] = "full_image_v1"
        var mixedEnvelope = admitted
        mixedEnvelope["version"] = 1
        var rawWithNested = legacy
        rawWithNested["receipt"] = legacy
        var tagWithoutBody = admitted
        tagWithoutBody["receipt"] = nil
        var bodyWithoutTag = admitted
        bodyWithoutTag["execution_contract"] = nil
        var invalid: [(String, Object)] = [
            ("legacy_wrong_version", body(legacy) { $0["version"] = 2 }),
            ("legacy_wrong_domain", body(legacy) { $0["domain"] = "zmux.local-zellij.terminal.v2" }),
            ("legacy_with_v2_observation", body(legacy) { $0["execution_observation"] = ["kind": "not_observed"] }),
            ("unknown_contract", wrongContract), ("mixed_envelope", mixedEnvelope),
            ("raw_with_nested_body", rawWithNested), ("tag_without_body", tagWithoutBody),
            ("body_without_tag", bodyWithoutTag),
            ("v2_body_wrong_version", body(admitted) { $0["version"] = 1 }),
            ("v2_body_future_version", body(admitted) { $0["version"] = 3 }),
            ("v2_body_wrong_domain", body(admitted) { $0["domain"] = "zmux.local-zellij.terminal.v1" }),
            ("v2_missing_observation", body(admitted) { $0["execution_observation"] = nil }),
            ("v2_unknown_observation", body(admitted) { $0["execution_observation"] = ["kind": "exec_succeeded"] }),
            ("v2_not_observed_with_failure", body(admitted) { $0["execution_observation"] = ["kind": "not_observed", "errno": 2] }),
            ("v2_shell_observed_with_failure", body(admitted) { $0["execution_observation"] = ["kind": "shell_observed", "stage": "configured_shell_exec"] }),
            ("v2_exec_failure_wrong_stage", body(failed) { $0["execution_observation"] = ["kind": "exec_failed", "stage": "user_command", "errno": 2] }),
            ("v2_exec_failure_zero_errno", body(failed) { $0["execution_observation"] = ["kind": "exec_failed", "stage": "configured_shell_exec", "errno": 0] }),
            ("v2_exec_failure_negative_errno", body(failed) { $0["execution_observation"] = ["kind": "exec_failed", "stage": "configured_shell_exec", "errno": -1] }),
            ("v2_exec_failure_overflow_errno", body(failed) { $0["execution_observation"] = ["kind": "exec_failed", "stage": "configured_shell_exec", "errno": 2_147_483_648] }),
            ("v2_exec_failure_missing_errno", body(failed) { $0["execution_observation"] = ["kind": "exec_failed", "stage": "configured_shell_exec"] })
        ]
        for (contract, wire) in [("v1", legacy), ("v2", admitted)] {
            for (kind, value) in [("exited", -1), ("exited", 256), ("signaled", 0), ("signaled", 128)] {
                invalid.append(("\(contract)_\(kind)_\(value)", body(wire) { $0["workload_exit"] = ["kind": kind, "value": value] }))
            }
            invalid.append(("\(contract)_unknown_exit_kind", body(wire) { $0["workload_exit"] = ["kind": "stopped", "value": 1] }))
            invalid.append(("\(contract)_exit_value_type", body(wire) { $0["workload_exit"] = ["kind": "exited", "value": "7"] }))
            invalid.append(("\(contract)_short_digest", body(wire) { $0["binding_digest"] = String(repeating: "a", count: 63) }))
            invalid.append(("\(contract)_noncanonical_digest", body(wire) { $0["binding_digest"] = String(repeating: "A", count: 64) }))
        }
        var rejected: [String] = []
        for (name, wire) in invalid {
            do {
                _ = try decode(wire)
                throw HistoryContractFailure(message: "Actual Models accepted malformed history: " + name)
            } catch is DecodingError {
                rejected.append(name)
            }
        }
        let result: Object = ["schema_version": 1, "production_models_decoder": true,
            "fixture_only_no_live_authority": true, "accepted": accepted,
            "accepted_exit_boundaries": acceptedBoundaries, "rejected": rejected]
        print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
    }
}
