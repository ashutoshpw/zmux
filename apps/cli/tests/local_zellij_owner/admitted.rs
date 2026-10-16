//! V2 authenticates the admitted direct Child across a legitimate arbitrary exec.
use super::*;
use zmux_session_runtime::child::{ProcessLifetime, process_lifetime};

#[test]
fn arbitrary_exec_keeps_the_admitted_child_attachable_and_retains_its_actual_exit() {
    let f = Fixture::new();
    let python = Command::new("python3")
        .args([
            "-c",
            "import os,sys;print(os.path.realpath(sys.executable))",
        ])
        .output()
        .unwrap();
    assert!(python.status.success());
    let python = String::from_utf8(python.stdout).unwrap().trim().to_owned();
    let counter = f.root.join("exec-count");
    let proof = f.root.join("one-input");
    let workload_identity = f.root.join("workload-identity.json");
    let script = f.root.join("arbitrary.py");
    fs::write(&script, format!("import json,os,pathlib,sys\nwith open({counter:?},'ab') as f:f.write(b'x')\npathlib.Path({identity:?}).write_text(json.dumps({{'pid':os.getpid(),'parent_pid':os.getppid()}}))\nprint('ADMITTED-EXEC-READY',flush=True)\nline=sys.stdin.readline()\npathlib.Path({proof:?}).write_text(line)\nsys.exit(7)\n",counter=counter.to_str().unwrap(),proof=proof.to_str().unwrap(),identity=workload_identity.to_str().unwrap())).unwrap();
    let command = format!("exec '{python}' -u '{}'", script.display());
    let mut first = Client::with_environment(
        &f,
        &[
            "local-zellij",
            "start",
            "exec-work",
            "--headless",
            "--command",
            &command,
        ],
        &[("SHELL", "/bin/sh".into())],
    );
    // Fixture-only input admission; this render is not product input readiness.
    first.rendered("ADMITTED-EXEC-READY");
    let record = f.records().unwrap()[0].clone();
    let id = record["id"].as_str().unwrap();
    assert_eq!(
        record["completion"]["execution_contract"],
        "admitted_lifetime_v2"
    );
    let binding = &record["completion"]["admission"];
    let lifetime: ProcessLifetime = serde_json::from_value(binding["lifetime"].clone()).unwrap();
    let current = process_identity(lifetime.pid).unwrap();
    assert_eq!(ProcessLifetime::from(&current), lifetime);
    let reported: Value = serde_json::from_slice(&fs::read(&workload_identity).unwrap()).unwrap();
    assert_eq!(reported["pid"], lifetime.pid);
    assert_eq!(reported["parent_pid"], lifetime.parent_pid);
    // A Python launcher can legitimately re-exec its app-bundle binary on
    // Darwin. The final image is evidence; it need not equal the invocation path.
    assert!(current.executable.is_absolute());
    let workload_image = current.executable.clone();
    assert_ne!(
        serde_json::to_value(&current.executable).unwrap(),
        binding["invocation"]["shell"]
    );
    assert_ne!(
        serde_json::to_value(&current.executable).unwrap(),
        binding["workload_birth"]["executable"]
    );
    eprintln!(
        "R03 arbitrary exec identity: {}",
        serde_json::json!({"invocation":python,"kernel_image":workload_image,"reported":reported,"admitted_lifetime":lifetime})
    );
    kill(Pid::from_raw(first.pid() as i32), Signal::SIGTERM).unwrap();
    assert_eq!(
        first.finish(),
        0,
        "{}",
        String::from_utf8_lossy(&first.output)
    );
    assert_eq!(process_lifetime(lifetime.pid).unwrap(), lifetime);
    let status = f.output(&["local-zellij", "status", "--id", id]);
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["state"], "identity_present");
    let mut second = Client::new(&f, &["local-zellij", "attach", "--headless", "--id", id]);
    second.rendered("ADMITTED-EXEC-READY");
    assert_eq!(process_lifetime(lifetime.pid).unwrap(), lifetime);
    assert_eq!(
        process_identity(lifetime.pid).unwrap().executable,
        workload_image
    );
    second.send(b"one-admitted-input\r");
    assert_eq!(
        second.finish(),
        7,
        "{}",
        String::from_utf8_lossy(&second.output)
    );
    assert_eq!(fs::read_to_string(proof).unwrap(), "one-admitted-input\n");
    assert_eq!(fs::read(&counter).unwrap(), b"x");
    let status = f.output(&["local-zellij", "status", "--id", id]);
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["completion_authenticated"], true);
    assert_eq!(status["consumption_confirmed"], true);
    let terminal = &status["terminal_receipt"];
    assert_eq!(terminal["execution_contract"], "admitted_lifetime_v2");
    assert_eq!(terminal["receipt"]["binding"], *binding);
    assert_eq!(
        terminal["receipt"]["workload_exit"],
        serde_json::json!({"kind":"exited","value":7})
    );
    let generation = binding["owner_generation"].as_str().unwrap();
    let cleanup = f.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        id,
        "--generation",
        generation,
    ]);
    assert!(
        cleanup.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    assert_eq!(fs::read(counter).unwrap(), b"x");
}
