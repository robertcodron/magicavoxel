//! MCP (Model Context Protocol) server for MagicaVoxel Rust Edition.
//!
//! Lets AI assistants build voxel art through JSON-RPC 2.0 over stdio: create
//! projects, sculpt with high-level primitives, manage palette / materials, compose
//! multi-object scenes, animate, run voxel shaders, render previews and export.
//!
//! Every user-facing feature of the editor must also be reachable here (see AGENTS.md).

pub mod json;
pub mod primitives;
pub mod session;
pub mod tools;

pub use json::JsonValue;
pub use session::McpSession;
