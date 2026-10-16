"""Original MIT readiness package contract; legacy admission remains separate."""
import json
from pathlib import Path
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
VARIANT = "input-readiness-v1"
SOURCES = {
    "input-readiness-reporter.rs": "reporter.rs",
    "patches/0002-optional-input-readiness.patch": "input-readiness.patch",
    "input-readiness-protocol.json": "protocol.json",
    "input-readiness-source.json": "source-inventory.json",
}


def require(condition, message):
    if not condition:
        raise RuntimeError("readiness package: " + message)


def read_json(path, limit=8 * 1024 * 1024):
    require(path.is_file() and path.stat().st_size <= limit, "bounded JSON missing/oversized")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON field")
            result[key] = value
        return result
    return json.loads(path.read_text(), object_pairs_hook=unique)


def verify_owner(producer_path, built, binary, owner_path, digest):
    require(owner_path is not None, "actual readiness owner receipt required")
    witness = read_json(owner_path)
    inventory_path = HERE / "input-readiness-source.json"
    inventory = read_json(inventory_path)
    extension = built.get("input_readiness", {})
    require(extension.get("requested") is True and extension.get("built") is True,
            "actual built readiness capability required")
    require(extension["inventory_sha256"] == digest(inventory_path)
            and extension["inventory"] == inventory, "producing source inventory differs")
    for name, expected in inventory["inputs_sha256"].items():
        require(digest(HERE / name) == expected, "producing source input changed: " + name)
    source = producer_path.parent / "source" / ("zellij-" + built["lock"]["source_commit"])
    for name, expected in inventory["upstream_files_sha256"].items():
        require(digest(source / name) == expected, "compiled source changed: " + name)
    protocol = read_json(HERE / "input-readiness-protocol.json")
    require(extension["capability_stdout"] == protocol["capability_stdout"]
            and extension["capability_home_unchanged"] is True, "capability evidence differs")
    require(witness["status"] == "passed" and witness["retained_source_unchanged"] is True
            and witness["producer_receipt_sha256"] == digest(producer_path)
            and witness["binary_sha256"] == built["binary_sha256"] == digest(binary),
            "actual owner did not test this exact producer executable")
    require(witness["producer_inventory_sha256"] == witness["current_inventory_sha256"]
            == digest(inventory_path), "actual owner source inventory differs")
    require(witness["executed_fixture_sha256"] == digest(HERE / "input-readiness-owner.py")
            == witness["fixture_sha256_at_producer"], "actual owner fixture changed")
    expected_inputs = {name: digest(HERE / name) for name in
                       ("input-readiness-owner.py", "run.py", "public_identity.c", "pty_exec.c")}
    require(witness["inputs_sha256"] == expected_inputs, "actual owner inputs changed")
    require(set(witness["outputs_sha256"]) == {"pty-0.log", "pty-1.log"}, "actual owner logs missing")
    for name, expected in witness["outputs_sha256"].items():
        path = owner_path.parent / name
        require(path.is_file() and path.stat().st_size <= 512 * 1024
                and digest(path) == expected, "actual owner log changed: " + name)
    lines = witness["exact_lines"]
    require(len(lines) == 2 and len(set(lines)) == 2
            and witness["workload_startup_count"] == 1 and witness["backend_gone"] is True
            and witness["workload_gone"] is True, "silent workload or cleanup evidence differs")
    require(len(witness["attachments"]) == 2, "two actual attachments required")
    for index, attachment in enumerate(witness["attachments"]):
        frontend, peer = attachment["frontend"], attachment["kernel_peer"]
        require(type(frontend["pid"]) is int and frontend["pid"] > 0
                and frontend["uid"] >= 0 and frontend["start_a"] > 0 and frontend["boot"]
                and Path(frontend["executable"]) == binary
                and peer["pid"] == frontend["pid"] and peer["uid"] == frontend["uid"],
                "actual retained frontend/kernel identity differs")
        require(type(attachment["input_writes"]) is int and attachment["input_writes"] == 1
                and attachment["readiness_source"] == "authenticated_control_only"
                and attachment["accepted_sequence"] == 3 and attachment["reporter_eof"] is True
                and attachment["frontend_alive_after_eof"] is True
                and attachment["child_reaped_exit_code"] == 0
                and 0 < attachment["input_ready_monotonic_ms"] < attachment["deadline_monotonic_ms"],
                "actual readiness/EOF/one-write/cleanup evidence differs")
        payload = bytes.fromhex(attachment["line_utf8_hex"])
        require(1 <= len(payload) <= 4096 and b"\n" not in payload and b"\r" not in payload
                and lines[index] == (payload + b"\n").hex()
                and attachment["workload_lines"] == lines[:index + 1], "exact silent input changed")
    return {"variant": VARIANT, "protocol_sha256": digest(HERE / "input-readiness-protocol.json"),
            "source_inventory_sha256": digest(inventory_path),
            "owner_receipt_sha256": digest(owner_path), "owner_receipt": str(owner_path)}


def retain_sources(destination, built, digest):
    """Keep upstream filtered bytes unchanged; add corresponding original code."""
    import shutil
    directory = destination / "SOURCES" / VARIANT
    directory.mkdir()
    retained = {}
    for original, name in SOURCES.items():
        target = directory / name
        shutil.copy2(HERE / original, target)
        retained[str(target.relative_to(destination))] = digest(target)
    require(read_json(directory / "source-inventory.json") == built["input_readiness"]["inventory"],
            "retained source inventory differs")
    return retained


def verify_sources(destination, built, producer):
    """Reconstruct the four compiled files from supplied corresponding source."""
    from package_sources import checked_members
    directory = destination / "SOURCES" / VARIANT
    inventory = read_json(directory / "source-inventory.json")
    require(inventory == built["input_readiness"]["inventory"], "corresponding source inventory differs")
    for original, name in SOURCES.items():
        expected = producer.digest(HERE / original)
        require(producer.digest(directory / name) == expected, "corresponding source changed: " + name)
    top = "zellij-" + built["lock"]["source_commit"]
    # The temporary reconstruction stays outside the payload and never compiles.
    with tempfile.TemporaryDirectory(prefix="readiness-source-", dir=destination.parent) as scratch:
        with tarfile.open(destination / "SOURCES/zellij-source.tar.gz") as archive:
            checked_members(archive, top)
            archive.extractall(scratch, filter="data")
        source = Path(scratch) / top
        producer.run(["patch", "--batch", "--forward", "-p1", "-i",
                      directory.resolve() / "input-readiness.patch"], source, {},
                     destination.parent / "readiness-source-reconstruction.log", 30)
        (source / "zellij-client/src/input_readiness.rs").write_bytes((directory / "reporter.rs").read_bytes())
        for name, expected in inventory["upstream_files_sha256"].items():
            require(producer.digest(source / name) == expected, "supplied source cannot reconstruct: " + name)
    return {str((directory / name).relative_to(destination)): producer.digest(directory / name)
            for name in SOURCES.values()}


def verify_consumer(owner_path):
    """Require named, executed observer4 behaviors plus their actual receipts."""
    import re
    import uuid
    text = (owner_path.parent / "owner-2.log").read_text()
    required = {
        "native::actual_typed_input_ready_delivers_one_silent_first_line_per_attachment",
        "native::actual_terminal_completion_before_input_ready_saves_and_joins_without_interactive_admission",
        "native::unsupported_readiness_capability_refuses_before_any_owner_reservation",
    }
    executed = set(re.findall(r"^test ([^ ]+) \.\.\. ok$", text, re.MULTILINE))
    require(required <= executed, "named observer4 consumer behaviors did not execute")
    ready, terminal = [], []
    for line in text.splitlines():
        if line.startswith("R03 typed input readiness: "):
            ready.append(json.loads(line.split(": ", 1)[1]))
        if line.startswith("R03 completion before input readiness: "):
            terminal.append(json.loads(line.split(": ", 1)[1]))
    require(len(ready) == 1 and len(terminal) == 2, "actual observer4 semantic receipts missing/ambiguous")
    row = ready[0]
    require(row["input_admission"] == "typed_input_ready" and row["workload_exit"] == 7
            and row["startup_count"] == 1 and all(row[key] is True for key in
                ("silent_workload", "first_line_once", "second_line_once", "actual_saved_completion", "actual_frontend_session_joined")),
            "actual silent observer4 admission evidence differs")
    first, second = uuid.UUID(row["first_attachment"]), uuid.UUID(row["second_attachment"])
    require(first.int != 0 and second.int != 0 and first != second, "observer4 attachments not distinct")
    require({row["workload_exit"] for row in terminal} == {0, 7}
            and all(row["input_ready"] is False and row["actual_saved_completion"] is True
                    and row["actual_frontend_session_joined"] is True and row["startup_count"] == 1 for row in terminal),
            "actual completion-before-Ready evidence differs")
    return {"executed": sorted(required), "silent_ready": ready[0], "terminal_before_ready": terminal}


def verify_archive(archive, manifest, built, digest, *, promotion_ready=False):
    """Check the emitted archive itself, including all corresponding source."""
    import hashlib
    from package_admission import MAX_ARCHIVE, archive_files
    require(archive.is_file() and archive.stat().st_size <= MAX_ARCHIVE, "archive exceeds bound")
    files = archive_files(archive.read_bytes())
    actual = json.loads(files["manifest.json"])
    require(actual.get("variant") == VARIANT, "archive variant differs")
    inventory = built["input_readiness"]["inventory"]
    for original, name in SOURCES.items():
        path = "SOURCES/" + VARIANT + "/" + name
        require(path in files, "corresponding source missing: " + name)
        expected = digest(HERE / original)
        require(hashlib.sha256(files[path]).hexdigest() == expected, "corresponding source differs: " + name)
    require(actual["input_readiness"]["source_inventory_sha256"] == built["input_readiness"]["inventory_sha256"]
            and actual["input_readiness"]["protocol_sha256"] == inventory["inputs_sha256"]["input-readiness-protocol.json"],
            "archive protocol/source inventory differs")
    require(type(promotion_ready) is bool and actual == manifest
            and actual["promotion_ready"] is promotion_ready, "archive manifest or promotion state differs")
    if promotion_ready:
        require(actual.get("diagnostic_only") is True
                and actual["input_readiness"].get("asserted_reviewed_source_inventory_sha256")
                    == built["input_readiness"]["inventory_sha256"],
                "promoted readiness archive lacks diagnostic scope or exact source review")
    require(set(files) == set(manifest["files"]) | {"manifest.json"}, "archive file inventory differs")
    for name, expected in manifest["files"].items():
        require(len(files[name]) == expected["bytes"] and hashlib.sha256(files[name]).hexdigest() == expected["sha256"],
                "archive member digest/size differs: " + name)
    require(hashlib.sha256(files["bin/zellij"]).hexdigest() == built["binary_sha256"], "archive executable differs")
    return {"variant": VARIANT, "corresponding_source_files": len(SOURCES), "members": len(files)}
