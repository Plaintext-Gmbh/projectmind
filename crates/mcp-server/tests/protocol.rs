// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration tests that drive the binary over its real stdio JSON-RPC channel.
//!
//! These tests build the binary (`cargo test` triggers the build), spawn it as a child
//! process, write JSON-RPC requests to its stdin and parse responses from its stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_projectmind-mcp"))
}

struct Server {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Server {
    fn spawn() -> Self {
        Self::spawn_env(None)
    }

    /// Spawn the server with an isolated `$PROJECTMIND_STATE` so a test that
    /// exercises a state-publishing tool (artifacts, walk-throughs) doesn't
    /// touch the developer's real cache directory.
    fn spawn_with_state(state: &Path) -> Self {
        Self::spawn_env(Some(state))
    }

    fn spawn_env(state: Option<&Path>) -> Self {
        let mut cmd = Command::new(binary_path());
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("PROJECTMIND_LOG", "error");
        if let Some(state) = state {
            cmd.env("PROJECTMIND_STATE", state);
        }
        let mut child = cmd.spawn().expect("spawn binary");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn call(&mut self, msg: &str) -> serde_json::Value {
        writeln!(self.stdin, "{msg}").expect("write stdin");
        self.stdin.flush().expect("flush stdin");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read stdout");
        serde_json::from_str(&line).expect("parse response")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn initialize_returns_server_info() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["serverInfo"]["name"], "projectmind-mcp");
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn initialize_returns_routing_instructions() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let instructions = resp["result"]["instructions"]
        .as_str()
        .expect("initialize must surface server instructions for client-side routing");
    assert!(instructions.contains("Desktop GUI"));
    assert!(instructions.contains("open_browser_repo"));
    assert!(instructions.contains("walkthrough_start"));
}

#[test]
fn tools_list_includes_open_repo() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let names: Vec<&str> = resp["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"open_repo"));
    assert!(names.contains(&"show_class"));
    assert!(names.contains(&"show_diagram"));
    assert!(names.contains(&"docs_for_class"));
}

#[test]
fn tools_list_includes_self_demo() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = resp["result"]["tools"].as_array().unwrap();
    let sd = tools
        .iter()
        .find(|t| t["name"] == "self_demo")
        .expect("self_demo tool registered");
    // Both knobs are optional and documented.
    let props = &sd["inputSchema"]["properties"];
    assert!(props.get("top").is_some(), "top documented");
    assert!(props.get("persona").is_some(), "persona documented");
    assert!(
        sd["inputSchema"].get("required").is_none(),
        "self_demo has no required args"
    );
}

#[test]
fn tools_list_includes_scaffold_c4_model() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = resp["result"]["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|t| t["name"] == "scaffold_c4_model")
        .expect("scaffold_c4_model tool registered");
    // No-argument tool: the schema declares no required fields.
    assert!(
        tool["inputSchema"].get("required").is_none(),
        "scaffold_c4_model has no required args"
    );
    // Its description must call out the non-clobber contract (#142).
    let descr = tool["description"].as_str().unwrap();
    assert!(
        descr.contains("NEVER clobbers"),
        "non-clobber contract documented"
    );
}

#[test]
fn tools_list_includes_merge_c4_model() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = resp["result"]["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|t| t["name"] == "merge_c4_model")
        .expect("merge_c4_model tool registered");
    // No-argument tool: the schema declares no required fields.
    assert!(
        tool["inputSchema"].get("required").is_none(),
        "merge_c4_model has no required args"
    );
    // Its description must call out the additive, edit-preserving contract (#142).
    let descr = tool["description"].as_str().unwrap();
    assert!(
        descr.contains("ADDITIVE"),
        "additive contract documented: {descr}"
    );
    assert!(
        descr.contains("preserv"),
        "edit-preservation documented: {descr}"
    );
}

#[test]
fn docs_for_class_returns_ranked_mentions() {
    let tmp = TempRepo::create_with_java_class();
    // One doc that links the source file, names the FQN and code-spans the
    // simple name; `Hello` (5 chars, one hump) is NOT distinctive, so the
    // bare name alone must not create hits.
    std::fs::write(
        tmp.root.join("DESIGN.md"),
        "# Design\n\nThe `Hello` entry point lives in [source](Hello.java); see demo.Hello.\nHello is also written bare here.\n",
    )
    .unwrap();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"docs_for_class","arguments":{"fqn":"demo.Hello"}}}"#,
    );
    let arr: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly the DESIGN.md hit: {arr:?}");
    assert_eq!(arr[0]["rel"], "DESIGN.md");
    assert_eq!(arr[0]["title"], "Design");
    // Best rule wins: the source-file link outranks fqn/code-span.
    assert_eq!(arr[0]["kind"], "link");
    // link (href) + fqn + code span = 3; the bare `Hello` mentions must not count.
    assert_eq!(arr[0]["count"], 3);
    assert_eq!(arr[0]["line"], 3);
}

#[test]
fn docs_for_class_rejects_unknown_fqn() {
    let tmp = TempRepo::create_with_java_class();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"docs_for_class","arguments":{"fqn":"nope.Missing"}}}"#,
    );
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("class not found"));
}

#[test]
fn unknown_method_returns_error_with_id() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":3,"method":"does/not/exist"}"#);
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32601);
    assert_eq!(resp["id"], 3);
}

#[test]
fn notification_yields_no_response_then_subsequent_request_works() {
    let mut s = Server::spawn();
    // Send a notification (no id) — server must not reply.
    writeln!(
        s.stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    s.stdin.flush().unwrap();
    // Then a real request — must produce a response.
    let resp = s.call(r#"{"jsonrpc":"2.0","id":42,"method":"ping"}"#);
    assert_eq!(resp["id"], 42);
    assert!(resp["result"].is_object());
}

#[test]
fn open_repo_then_repo_info_round_trips() {
    let tmp = TempRepo::create_with_java_class();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    );
    let resp = s.call(&req);
    assert!(
        resp["error"].is_null() || resp["error"].is_null(),
        "unexpected error: {resp}"
    );
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["modules"], 1);
    assert_eq!(payload["classes"], 1);

    let resp2 =
        s.call(r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"repo_info"}}"#);
    let payload2: serde_json::Value =
        serde_json::from_str(resp2["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload2["modules"], 1);
    assert_eq!(payload2["classes"], 1);
}

#[test]
fn list_classes_filters_by_stereotype() {
    let tmp = TempRepo::create_with_spring_service();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_classes","arguments":{"stereotype":"service"}}}"#,
    );
    let arr: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["fqn"], "demo.UserService");
    assert_eq!(arr[0]["stereotypes"][0], "service");
}

#[test]
fn find_class_returns_matches() {
    let tmp = TempRepo::create_with_spring_service();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_class","arguments":{"query":"user"}}}"#,
    );
    let arr: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["fqn"], "demo.UserService");
}

#[test]
fn class_outline_returns_methods_and_fields() {
    let tmp = TempRepo::create_with_class_outline();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"class_outline","arguments":{"fqn":"demo.Sample"}}}"#,
    );
    let outline: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(outline["fqn"], "demo.Sample");
    let methods = outline["methods"].as_array().unwrap();
    assert!(methods.iter().any(|m| m["name"] == "doIt"));
    let fields = outline["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["name"] == "counter"));
}

#[test]
fn module_summary_includes_stereotype_counts() {
    let tmp = TempRepo::create_with_spring_service();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"module_summary"}}"#,
    );
    let modules: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let modules = modules.as_array().unwrap();
    assert_eq!(modules.len(), 1);
    assert!(modules[0]["stereotypes"]["service"].as_u64().unwrap() >= 1);
}

#[test]
fn show_diagram_bean_graph_returns_mermaid() {
    let tmp = TempRepo::create_with_spring_service();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"show_diagram","arguments":{"type":"bean-graph"}}}"#,
    );
    let mermaid = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(mermaid.starts_with("flowchart LR\n"));
}

#[test]
fn view_file_rejects_paths_outside_open_repo() {
    let tmp = TempRepo::create_with_java_class();
    let outside = TempRepo::create_with_text_file("secret.txt", "secret");
    let mut s = Server::spawn();
    let repo_path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{repo_path}"}}}}}}"#
    ));

    let outside_path = outside
        .root
        .join("secret.txt")
        .to_string_lossy()
        .into_owned();
    let resp = s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"view_file","arguments":{{"path":"{outside_path}"}}}}}}"#
    ));

    assert_eq!(resp["error"]["code"], -32602);
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("outside repository"),
        "unexpected response: {resp}"
    );
}

#[test]
fn pattern_check_rejects_unknown_pattern() {
    let tmp = TempRepo::create_with_java_class();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"pattern_check","arguments":{"pattern":"made_up"}}}"#,
    );
    assert_eq!(resp["error"]["code"], -32602);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown pattern"));
}

#[test]
fn pattern_check_layered_returns_result_envelope() {
    let tmp = TempRepo::create_with_java_class();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"pattern_check","arguments":{"pattern":"layered"}}}"#,
    );
    let body: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["pattern"], "layered");
    assert!(body["holds"].is_array());
    assert!(body["violations"].is_array());
    assert!(body["confidence"].is_number());
}

#[test]
fn risk_atlas_returns_envelope_with_window_and_weights() {
    let tmp = TempRepo::create_git_repo_with_class();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"risk_atlas","arguments":{"top":5}}}"#,
    );
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let body: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["window_days"], 90);
    assert!(body["weights"]["churn"].is_number());
    // Coverage envelope present (null when no report) and scores array.
    assert!(body.get("coverage").is_some(), "missing coverage envelope");
    assert!(body["scores"].is_array());
    let scores = body["scores"].as_array().unwrap();
    assert!(!scores.is_empty(), "expected at least one scored class");
    let first = &scores[0];
    for k in [
        "fqn", "module", "file", "score", "churn", "cx", "cov", "fan_in", "fan_out", "sloc", "why",
    ] {
        assert!(first.get(k).is_some(), "missing field {k} in score entry");
    }
    // No coverage report in this fixture → cov is null, degrades gracefully.
    assert!(
        first["cov"].is_null(),
        "cov should be null without a report"
    );
    assert!(body["coverage"].is_null(), "coverage meta should be null");
}

#[test]
fn risk_atlas_surfaces_coverage_from_jacoco_report() {
    let tmp = TempRepo::create_git_repo_with_jacoco();
    let mut s = Server::spawn();
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"risk_atlas","arguments":{"top":5}}}"#,
    );
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let body: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    // Coverage report detected → meta populated.
    assert_eq!(body["coverage"]["format"], "jacoco");
    let scores = body["scores"].as_array().unwrap();
    let hello = scores
        .iter()
        .find(|s| s["fqn"] == "demo.Hello")
        .expect("demo.Hello scored");
    // JaCoCo report says 8 covered / 2 missed → 0.8.
    let cov = hello["cov"].as_f64().expect("cov populated from report");
    assert!((cov - 0.8).abs() < 1e-6, "expected 0.8, got {cov}");
}

#[test]
fn present_artifact_stores_html_and_pushes_intent() {
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    // HTML artifact carrying a script tag — must be stored verbatim (inert),
    // never executed or stripped.
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"present_artifact","arguments":{"title":"XSS Probe","format":"html","content":"<h1>Report</h1><script>alert(1)</script>"}}}"#,
    );
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["format"], "html");
    let id = payload["id"].as_str().unwrap().to_string();
    assert_eq!(
        id, "xss-probe",
        "id derived as a stable slug from the title"
    );

    // The statefile now carries the artifact view intent (kebab-case tag).
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("current.json")).unwrap()).unwrap();
    assert_eq!(state["view"]["kind"], "artifact");
    assert_eq!(state["view"]["id"], id);

    // Body persisted next to the statefile, script intact but inert.
    let body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("artifacts").join(format!("{id}.json"))).unwrap(),
    )
    .unwrap();
    assert!(body["content"]
        .as_str()
        .unwrap()
        .contains("<script>alert(1)</script>"));

    // Re-presenting the same id replaces in place (markdown this time).
    s.call(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"present_artifact","arguments":{"id":"xss-probe","title":"XSS Probe v2","format":"markdown","content":"**hi**"}}}"#,
    );

    // list_artifacts surfaces exactly one entry with the updated metadata.
    let resp2 = s.call(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_artifacts"}}"#,
    );
    let arr: serde_json::Value =
        serde_json::from_str(resp2["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1, "same id replaces, not appends");
    assert_eq!(arr[0]["id"], id);
    assert_eq!(arr[0]["title"], "XSS Probe v2");
    assert_eq!(arr[0]["format"], "markdown");
    assert!(arr[0]["size"].is_number());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn present_artifact_rejects_unknown_format() {
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"present_artifact","arguments":{"title":"X","format":"pdf","content":"x"}}}"#,
    );
    assert_eq!(resp["error"]["code"], -32602);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown format"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn walkthrough_start_stamps_schema_v2_and_accepts_new_kinds() {
    // Cockpit 2.4 (#160): a tour authored with risk / pattern / atlas steps
    // must round-trip through walkthrough_start, and the on-disk body must be
    // stamped schemaVersion 2.
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let start = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"walkthrough_start","arguments":{"title":"2.4 tour","steps":[{"title":"Risk","target":{"kind":"risk","fqn":"a.b.C","focus":"validateToken","show":["churn","cx","cov"]}},{"title":"Pattern","target":{"kind":"pattern","pattern":"Repository","scope":"module:auth","view":"violations"}},{"title":"Atlas","target":{"kind":"atlas","module":"auth","highlight_fqns":["a.b.C"]}}]}}}"#;
    let resp = s.call(start);
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["total"], 3);

    // Append a fourth step of a new kind — walkthrough_append must accept it.
    let append = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"walkthrough_append","arguments":{"step":{"title":"More risk","target":{"kind":"risk","fqn":"a.b.D"}}}}}"#;
    let resp = s.call(append);
    assert!(resp["error"].is_null(), "append error: {resp}");
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["total"], 4);

    // The body persisted next to the statefile carries schemaVersion 2 and the
    // new kinds verbatim.
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("walkthrough.json")).unwrap())
            .unwrap();
    assert_eq!(body["schemaVersion"], 2);
    let steps = body["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 4);
    assert_eq!(steps[0]["target"]["kind"], "risk");
    assert_eq!(steps[0]["target"]["show"][0], "churn");
    assert_eq!(steps[1]["target"]["kind"], "pattern");
    assert_eq!(steps[1]["target"]["scope"], "module:auth");
    assert_eq!(steps[2]["target"]["kind"], "atlas");
    assert_eq!(steps[2]["target"]["highlight_fqns"][0], "a.b.C");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn walkthrough_append_rejects_unknown_kind() {
    // The union stays narrow: an unknown step kind is a params error, not a
    // silently dropped step.
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"walkthrough_start","arguments":{"title":"t","steps":[{"title":"n","target":{"kind":"note"}}]}}}"#,
    );
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"walkthrough_append","arguments":{"step":{"title":"x","target":{"kind":"bogus"}}}}}"#,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "expected invalid params: {resp}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tools_list_includes_walkthrough_query() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = resp["result"]["tools"].as_array().unwrap();
    let wq = tools
        .iter()
        .find(|t| t["name"] == "walkthrough_query")
        .expect("walkthrough_query tool registered");
    let schema = &wq["inputSchema"];
    assert!(schema["properties"].get("question").is_some());
    assert!(schema["properties"].get("prefer_tours").is_some());
    assert!(schema["properties"].get("top_k").is_some());
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"question"));
}

#[test]
fn walkthrough_query_requires_open_repo() {
    // Without an open repository the tool is a params error, like every
    // other repo-scoped tool.
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"walkthrough_query","arguments":{"question":"how does login work"}}}"#,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "expected invalid params without an open repo: {resp}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn walkthrough_query_returns_grep_fallback_without_embed_feature() {
    // The default build has no local embedding model, so the semantic
    // lookup must degrade gracefully: a well-formed envelope with
    // `fallback: "grep"`, never a crash. (With the `embed` feature this
    // same call would return matched tour steps instead — verified in CI.)
    let tmp = TempRepo::create_with_java_class();
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    // Author a tour so there is something that *could* be matched — the
    // point is that without a model we still answer with the grep hint.
    s.call(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"walkthrough_start","arguments":{"title":"Auth flow","steps":[{"title":"Login","narration":"login controller","target":{"kind":"note"}}]}}}"#,
    );
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"walkthrough_query","arguments":{"question":"how does login work"}}}"#,
    );
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let body: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        body["fallback"], "grep",
        "no embed feature → grep fallback: {body}"
    );
    assert!(body["steps"].as_array().unwrap().is_empty());
    assert_eq!(body["confidence"], 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tools_list_includes_architect_briefing() {
    let mut s = Server::spawn();
    let resp = s.call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = resp["result"]["tools"].as_array().unwrap();
    let ab = tools
        .iter()
        .find(|t| t["name"] == "architect_briefing")
        .expect("architect_briefing tool registered");
    assert!(ab["inputSchema"]["properties"].get("since").is_some());
}

#[test]
fn architect_briefing_requires_open_repo() {
    // Repo-scoped like the rest: no open repo → invalid params.
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"architect_briefing","arguments":{}}}"#,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "expected invalid params without an open repo: {resp}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn architect_briefing_rejects_bad_since() {
    let tmp = TempRepo::create_git_repo_with_class();
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"architect_briefing","arguments":{"since":"not-a-date!!"}}}"#,
    );
    assert_eq!(resp["error"]["code"], -32602, "unexpected: {resp}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn open_repo_writes_session_log_and_briefing_diffs_it() {
    // open_repo must append to `.projectmind/state/sessions.jsonl`, and the
    // briefing must read that history and return a well-formed envelope.
    let tmp = TempRepo::create_git_repo_with_class();
    let (mut s, dir) = spawn_isolated();
    s.call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let path = tmp.root.to_string_lossy().into_owned();
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));

    // The session log now exists with at least one JSON line.
    let log = tmp.root.join(".projectmind/state/sessions.jsonl");
    assert!(log.is_file(), "sessions.jsonl not written on open_repo");
    let contents = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "one snapshot per open_repo");
    let rec: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(rec["ts"].is_number());
    assert!(rec["top_classes"].is_array());
    assert!(rec["pattern_violations"].is_object());

    // Re-opening the same repo appends a second snapshot so the briefing has a
    // baseline to compare against.
    s.call(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"open_repo","arguments":{{"path":"{path}"}}}}}}"#
    ));
    let contents = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "second open_repo appends a snapshot");

    // architect_briefing returns the envelope with markdown + a session window.
    let resp = s.call(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"architect_briefing","arguments":{}}}"#,
    );
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    let body: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["sessions_recorded"], 2);
    let briefing = &body["briefing"];
    assert!(briefing["new_hotspots"].is_array());
    assert!(briefing["pattern_drift"].is_array());
    assert!(briefing["risk_delta"]["up"].is_array());
    assert!(briefing["risk_delta"]["down"].is_array());
    assert!(briefing["session_window"]["from"].is_number());
    assert!(briefing["session_window"]["to"].is_number());
    assert!(
        body["markdown"]
            .as_str()
            .unwrap()
            .contains("Architect Briefing"),
        "markdown embed present: {body}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_briefing_subcommand_prints_and_records() {
    // The `projectmind briefing --repo <path>` CLI face records a snapshot and
    // prints a plain-text briefing. Running it twice yields a session window.
    let tmp = TempRepo::create_git_repo_with_class();
    let path = tmp.root.to_string_lossy().into_owned();

    let run = || {
        let out = Command::new(binary_path())
            .args(["briefing", "--repo", &path])
            .env("PROJECTMIND_LOG", "error")
            .output()
            .expect("run briefing subcommand");
        assert!(
            out.status.success(),
            "briefing exited non-zero: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // First run: only one session → the "no baseline" note.
    let first = run();
    assert!(
        first.contains("Architect briefing"),
        "unexpected first output: {first}"
    );

    // Second run: now there is a baseline → a window line.
    let second = run();
    assert!(
        second.contains("Window:") || second.contains("Nothing got worse"),
        "unexpected second output: {second}"
    );

    // The session log recorded both runs.
    let log = tmp.root.join(".projectmind/state/sessions.jsonl");
    let contents = std::fs::read_to_string(&log).unwrap();
    let count = contents.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(count, 2, "each CLI run appends one snapshot");
}

// ----- helpers -----

/// Spawn a server against an isolated statefile directory, pre-seeding a fresh
/// heartbeat so the server treats the GUI as already running (no desktop-app
/// auto-launch during the test). Returns the server plus its state dir.
fn spawn_isolated() -> (Server, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "projectmind-art-it-{}-{}",
        std::process::id(),
        uniq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        dir.join("ui-heartbeat.json"),
        format!(r#"{{"pid":1,"ts":{ts}}}"#),
    )
    .unwrap();
    let server = Server::spawn_with_state(&dir.join("current.json"));
    (server, dir)
}

struct TempRepo {
    root: PathBuf,
}

impl TempRepo {
    fn create_with_java_class() -> Self {
        let root =
            std::env::temp_dir().join(format!("projectmind-it-{}-{}", std::process::id(), uniq()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Hello.java"),
            "package demo;\npublic class Hello {}\n",
        )
        .unwrap();
        Self { root }
    }

    fn create_with_spring_service() -> Self {
        let root =
            std::env::temp_dir().join(format!("projectmind-it-{}-{}", std::process::id(), uniq()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("UserService.java"),
            "package demo;\n@Service\npublic class UserService {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Plain.java"),
            "package demo;\npublic class Plain {}\n",
        )
        .unwrap();
        Self { root }
    }

    fn create_with_class_outline() -> Self {
        let root =
            std::env::temp_dir().join(format!("projectmind-it-{}-{}", std::process::id(), uniq()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Sample.java"),
            "package demo;\npublic class Sample {\n    private int counter;\n    public void doIt() {}\n}\n",
        )
        .unwrap();
        Self { root }
    }

    fn create_with_text_file(name: &str, contents: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("projectmind-it-{}-{}", std::process::id(), uniq()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(name), contents).unwrap();
        Self { root }
    }

    /// Builds a temporary directory that is also a real git repo with one
    /// commit touching `Hello.java`. Needed by `risk_atlas` which walks git
    /// history to compute churn.
    fn create_git_repo_with_class() -> Self {
        use std::process::Command;
        let root =
            std::env::temp_dir().join(format!("projectmind-it-{}-{}", std::process::id(), uniq()));
        std::fs::create_dir_all(&root).unwrap();
        let java = "package demo;\npublic class Hello {\n    public void greet(String name) {\n        if (name != null && !name.isEmpty()) {\n            System.out.println(\"hi \" + name);\n        }\n    }\n}\n";
        std::fs::write(root.join("Hello.java"), java).unwrap();
        // libgit2 requires a configured identity; pass via env rather than
        // touching the test runner's git config.
        let env = [
            ("GIT_AUTHOR_NAME", "Test"),
            ("GIT_AUTHOR_EMAIL", "test@example.com"),
            ("GIT_COMMITTER_NAME", "Test"),
            ("GIT_COMMITTER_EMAIL", "test@example.com"),
        ];
        let mut init = Command::new("git");
        init.args(["init", "-q"]).current_dir(&root);
        init.env("GIT_CONFIG_GLOBAL", "/dev/null");
        init.env("GIT_CONFIG_SYSTEM", "/dev/null");
        let init_ok = init.status().is_ok_and(|s| s.success());
        assert!(init_ok, "git init failed in temp repo {}", root.display());

        let mut add = Command::new("git");
        add.args(["add", "Hello.java"]).current_dir(&root);
        let _ = add.status();

        let mut commit = Command::new("git");
        commit
            .args(["commit", "-q", "-m", "seed"])
            .current_dir(&root);
        commit.env("GIT_CONFIG_GLOBAL", "/dev/null");
        commit.env("GIT_CONFIG_SYSTEM", "/dev/null");
        for (k, v) in env {
            commit.env(k, v);
        }
        let _ = commit.status();
        Self { root }
    }

    /// Like [`Self::create_git_repo_with_class`] but drops a JaCoCo report at
    /// `target/site/jacoco/jacoco.xml` reporting 80% line coverage for
    /// `demo.Hello`. Exercises the coverage loader end-to-end through the MCP
    /// `risk_atlas` tool.
    fn create_git_repo_with_jacoco() -> Self {
        let repo = Self::create_git_repo_with_class();
        let jdir = repo.root.join("target/site/jacoco");
        std::fs::create_dir_all(&jdir).unwrap();
        let xml = r#"<?xml version="1.0"?>
<report name="demo">
  <package name="demo">
    <class name="demo/Hello" sourcefilename="Hello.java">
      <counter type="LINE" missed="2" covered="8"/>
    </class>
  </package>
</report>"#;
        std::fs::write(jdir.join("jacoco.xml"), xml).unwrap();
        repo
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn uniq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
