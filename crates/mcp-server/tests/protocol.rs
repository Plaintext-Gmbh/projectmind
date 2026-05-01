// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration tests that drive the binary over its real stdio JSON-RPC channel.
//!
//! These tests build the binary (`cargo test` triggers the build), spawn it as a child
//! process, write JSON-RPC requests to its stdin and parse responses from its stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
        let mut child = Command::new(binary_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("PROJECTMIND_LOG", "error")
            .spawn()
            .expect("spawn binary");
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

// ----- helpers -----

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
