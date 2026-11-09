//! End-to-end protocol tests: spawn the real `mv-mcp` binary and talk JSON-RPC over stdio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use mv_mcp::JsonValue;

/// Run the server with `input` on stdin (closed afterwards), return (stdout lines, exit success).
fn run(input: &str) -> (Vec<String>, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mv-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mv-mcp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    (
        text.lines().map(str::to_string).collect(),
        out.status.success(),
    )
}

fn parse(line: &str) -> JsonValue {
    JsonValue::parse(line).unwrap_or_else(|e| panic!("invalid JSON on stdout ({e}): {line}"))
}

fn result(v: &JsonValue) -> &JsonValue {
    v.get("result")
        .unwrap_or_else(|| panic!("no result in {}", v.to_json_string()))
}

fn error_code(v: &JsonValue) -> i64 {
    v.get("error")
        .and_then(|e| e.get_i64("code"))
        .expect("error.code")
}

#[test]
fn initialize_and_notification() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        "\n",
    );
    let (lines, ok) = run(input);
    assert!(ok);
    // The notification must not produce a line: exactly two responses.
    assert_eq!(lines.len(), 2, "{lines:?}");
    let init = parse(&lines[0]);
    assert_eq!(init.get_i64("id"), Some(1));
    let r = result(&init);
    assert_eq!(r.get_str("protocolVersion"), Some("2024-11-05"));
    assert_eq!(
        r.get("serverInfo").unwrap().get_str("name"),
        Some("magicavoxel-mcp")
    );
    let caps = r.get("capabilities").unwrap();
    for c in ["tools", "resources", "prompts"] {
        assert!(caps.get(c).is_some(), "missing capability {c}");
    }
    let ping = parse(&lines[1]);
    assert_eq!(ping.get_i64("id"), Some(2));
    assert_eq!(ping.get_str("jsonrpc"), Some("2.0"));
    assert!(result(&ping).get("anything").is_none());
}

#[test]
fn tools_list_shape() {
    let (lines, ok) = run("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n");
    assert!(ok);
    assert_eq!(lines.len(), 1);
    let tools = result(&parse(&lines[0]))
        .get_array("tools")
        .expect("tools array")
        .to_vec();
    for t in &tools {
        assert!(t.get_str("name").is_some(), "{}", t.to_json_string());
        assert!(t.get_str("description").is_some(), "{}", t.to_json_string());
        assert!(
            matches!(t.get("inputSchema"), Some(JsonValue::Object(_))),
            "{}",
            t.to_json_string()
        );
    }
}

#[test]
fn resources_and_prompts() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"magicavoxel://project/guide"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"magicavoxel://nope"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"prompts/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":5,"method":"prompts/get","params":{"name":"create_voxel_object","arguments":{"subject":"a red dragon"}}}"#,
        "\n",
    );
    let (lines, ok) = run(input);
    assert!(ok);
    assert_eq!(lines.len(), 5);
    let list = parse(&lines[0]);
    let uris: Vec<&str> = result(&list)
        .get_array("resources")
        .unwrap()
        .iter()
        .filter_map(|r| r.get_str("uri"))
        .collect();
    assert!(uris.contains(&"magicavoxel://project/guide"));

    let read = parse(&lines[1]);
    let contents = result(&read).get_array("contents").unwrap();
    assert_eq!(
        contents[0].get_str("uri"),
        Some("magicavoxel://project/guide")
    );
    assert!(contents[0].get_str("text").unwrap().contains("Z-up"));

    assert_eq!(error_code(&parse(&lines[2])), -32602);

    let prompts = parse(&lines[3]);
    assert!(!result(&prompts).get_array("prompts").unwrap().is_empty());

    let got = parse(&lines[4]);
    let msgs = result(&got).get_array("messages").unwrap();
    let text = msgs[0].get("content").unwrap().get_str("text").unwrap();
    assert!(text.contains("a red dragon"));
}

#[test]
fn errors() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":7,"method":"no/such/method"}"#,
        "\n",
        "{this is not json\n",
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"definitely_not_a_tool","arguments":{}}}"#,
        "\n",
    );
    let (lines, ok) = run(input);
    assert!(ok);
    assert_eq!(lines.len(), 3);
    let unknown = parse(&lines[0]);
    assert_eq!(error_code(&unknown), -32601);
    assert_eq!(unknown.get_i64("id"), Some(7));
    let bad = parse(&lines[1]);
    assert_eq!(error_code(&bad), -32700);
    assert!(matches!(bad.get("id"), Some(JsonValue::Null)));
    let call = parse(&lines[2]);
    assert_eq!(call.get_i64("id"), Some(8));
    assert_eq!(result(&call).get_bool("isError"), Some(true));
}

#[test]
fn blank_lines_ignored_and_clean_exit_on_eof() {
    let (lines, ok) = run("\n\n   \n{\"jsonrpc\":\"2.0\",\"id\":\"abc\",\"method\":\"ping\"}\n\n");
    assert!(ok, "server must exit 0 on EOF");
    assert_eq!(lines.len(), 1);
    assert_eq!(parse(&lines[0]).get_str("id"), Some("abc"));
    // Empty input: no output, clean exit.
    let (lines, ok) = run("");
    assert!(ok);
    assert!(lines.is_empty());
}

#[test]
fn last_line_without_newline_and_interactive_flow() {
    // Interactive: write one request, read its response before sending the next.
    let mut child = Command::new(env!("CARGO_BIN_EXE_mv-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for i in 1..=3 {
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":{i},"method":"ping"}}"#).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(parse(line.trim()).get_i64("id"), Some(i));
    }
    // Final message without trailing newline is still processed.
    write!(stdin, r#"{{"jsonrpc":"2.0","id":4,"method":"ping"}}"#).unwrap();
    drop(stdin);
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).unwrap();
    assert_eq!(parse(rest.trim()).get_i64("id"), Some(4));
    assert!(child.wait().unwrap().success());
}

#[test]
fn help_and_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_mv-mcp"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("mcpServers") && text.contains("claude mcp add"));
    let out = Command::new(env!("CARGO_BIN_EXE_mv-mcp"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .starts_with("mv-mcp "));
}
