//! `mv-mcp` binary: MCP server over stdio (one JSON-RPC message per line).
//!
//! stdout carries protocol messages only; all logs go to stderr.

use std::io::{BufRead, Write};

const USAGE: &str = r#"mv-mcp - MCP (Model Context Protocol) server for MagicaVoxel Rust Edition

USAGE:
    mv-mcp [--help | --version]

Speaks newline-delimited JSON-RPC 2.0 over stdin/stdout (MCP stdio transport).
Logs are written to stderr. The server exits cleanly when stdin is closed.

Register it in an MCP client configuration (e.g. Claude Desktop, Cursor):

    {"mcpServers": {"magicavoxel": {"command": "/path/to/mv-mcp"}}}

Or with Claude Code:

    claude mcp add magicavoxel -- /path/to/mv-mcp
"#;

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return;
            }
            "--version" | "-V" => {
                println!("mv-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("mv-mcp: unknown argument '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    eprintln!(
        "mv-mcp {} ready: reading JSON-RPC messages from stdin",
        env!("CARGO_PKG_VERSION")
    );
    let mut session = mv_mcp::McpSession::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mv-mcp: failed to read stdin: {e}");
                std::process::exit(1);
            }
        };
        if let Some(response) = session.handle_jsonrpc(&line) {
            if let Err(e) = writeln!(out, "{response}").and_then(|_| out.flush()) {
                eprintln!("mv-mcp: failed to write stdout: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("mv-mcp: stdin closed, shutting down");
}
