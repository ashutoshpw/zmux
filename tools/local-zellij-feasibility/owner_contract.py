"""Frozen public owner behavior baseline and executed source evidence."""
import json
from pathlib import Path
import re

# Names identify the independently exercised behaviors, not an aggregate count.
REQUIRED_TESTS = {
    "owner-0": [
        "live_identity_contains_actual_parent_and_stable_boot_start_path",
        "repeated_exit_observation_retains_the_child_until_final_wait",
        "dropping_an_active_client_requests_cleanup_without_joining",
        "failed_start_releases_its_owned_resource_on_the_session_worker",
        "exited_session_rejects_new_input_and_resize_requests",
        "exited_process_history_remains_navigable_without_accepting_new_input",
        "input_and_resize_reach_the_real_terminal",
        "malformed_requests_do_not_change_session_geometry_or_reach_input",
        "output_flood_does_not_prevent_cancel_and_leader_reaping",
        "real_pty_output_is_parsed_and_process_exit_is_reported",
        "interactive_shell_preserves_exact_multiline_input_across_native_chunk_boundaries",
        "socket_password_scrubbing_wins_over_application_environment_overrides",
        "saturated_input_returns_backpressure_without_blocking_cancellation",
        "write_receipt_confirms_full_pty_acceptance_in_the_existing_input_order",
        "bounded_write_receipts_finish_once_under_backpressure_deadline_and_shutdown",
        "queued_write_deadline_cancels_before_a_blocked_predecessor_drains_without_replay",
        "graceful_deadline_escalates_only_the_retained_child_and_reports_forced_cleanup",
        "graceful_close_retains_wrapper_pty_and_resource_until_nested_cleanup_and_final_reap"
    ],
    "owner-1": [
        "local_zellij::broker::tests::rejected_real_socket_input_retains_hash_and_pre_send_cancellation_phase",
        "local_zellij::supervisor::tests::configured_bootstrap_waits_for_fragmented_frames_on_an_initially_nonblocking_socket",
        "local_zellij::broker::tests::real_socket_backpressure_preserves_order_and_half_close_with_bounded_buffers",
        "local_zellij::broker::tests::real_kernel_peers_reject_wrong_owned_child_before_connect_and_changed_backend_before_forwarding",
        "local_zellij::input_readiness::tests::actual_retained_peer_requires_accepted_eof_before_admission",
        "local_zellij::input_readiness::tests::actual_retained_peer_wrong_nonce_and_trailing_frame_never_admit",
        "local_zellij::input_readiness::tests::a_different_actual_direct_child_cannot_assert_readiness_for_the_retained_frontend",
        "local_zellij::input_readiness::tests::replacing_the_actual_readiness_socket_cannot_admit_a_frontend",
        "local_zellij::input_readiness::tests::fixed_monotonic_budget_and_changed_retained_identity_fail_closed"
    ],
    "owner-2": [
        "foreground_zellij_grammar_refuses_unsupported_lifecycle_before_any_owner",
        "production_supervisor_refuses_fifo_descriptor_without_waiting_for_a_writer",
        "production_supervisor_rejects_changed_starter_and_unauthorized_spawn",
        "queued_cancellation_during_registry_wait_cannot_authorize_a_workload",
        "real_foreground_bootstrap_detach_and_reattach_keep_the_original_owned_workload"
    ]
}


def owner_checks(*, toolchain=None):
    """One command owner for produced and app-bundled dependency validation."""
    cargo = ["cargo"] + ([toolchain] if toolchain else [])
    return [
        cargo + ["test", "--locked", "-p", "zmux-session-runtime", "--test", "child", "--test", "lifecycle", "--test", "shutdown", "--", "--nocapture"],
        # This ignored entrypoint is a child-process fixture, not an outer test.
        # Actual protocol tests above still spawn it with --exact --ignored.
        cargo + ["test", "--locked", "-p", "zmux-remote-runtime", "--lib", "local_zellij", "--", "--nocapture",
                 "--skip", "local_zellij::input_readiness::tests::retained_readiness_peer"],
        cargo + ["test", "--locked", "-p", "zmux", "--features", "local-zellij-conformance", "--test", "local_zellij", "--test", "local_zellij_owner", "--", "--nocapture"],
    ]


def source_hashes(root, digest):
    paths = {root / "Cargo.toml", root / "Cargo.lock"}
    for prefix in ("apps/cli", "crates", "vendor/darwin-libproc", ".cargo"):
        for path in (root / prefix).rglob("*"):
            if path.is_file() and (path.suffix == ".rs" or path.name in ("Cargo.toml", "config", "config.toml")):
                paths.add(path)
    if len(paths) > 2000:
        raise RuntimeError("owner source inventory exceeded file bound")
    return {str(path.relative_to(root)): digest(path) for path in sorted(paths)}


def verify_logs(directory):
    counts = []
    tested = {}
    journey = None
    for label, required in REQUIRED_TESTS.items():
        text = (directory / (label + ".log")).read_text()
        names = set(re.findall(r"^test ([^ ]+) \.\.\. ok$", text, re.MULTILINE))
        if not set(required) <= names:
            raise RuntimeError(f"required owner behavior not executed: {label}: {sorted(set(required) - names)}")
        summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
        expected = 3 if label == "owner-0" else 1 if label == "owner-1" else 2
        if len(summaries) != expected or any(int(failed) or int(ignored) for _, failed, ignored in summaries):
            raise RuntimeError("owner suites missing, failed or ignored")
        counts.extend(int(passed) for passed, _, _ in summaries)
        tested[label] = sorted(names)
        for line in text.splitlines():
            if line.startswith("R03 real owner receipt: "):
                if journey is not None:
                    raise RuntimeError("ambiguous actual owner journey evidence")
                journey = json.loads(line.split(": ", 1)[1])
    if journey is None or journey["startup_count"] != 1 or any(journey[key] is not True for key in ("actual_input", "resize", "stop_resume", "term_detach", "hup_detach", "reattach", "termios_restored")):
        raise RuntimeError("actual owner journey did not establish package prerequisites")
    return {"passed": sum(counts), "suite_counts": counts, "tested": tested, "journey": journey}
