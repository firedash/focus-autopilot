// SPDX-License-Identifier: Apache-2.0
//! The hook receiver (design sections 5, 6 and 13): a loopback HTTP server that checks every
//! request, answers `200` at once, and turns Claude Code hook payloads into
//! [`focus_core::AgentEvent`]s. No UI and no OS calls, so it runs headless.
#![forbid(unsafe_code)]

pub mod check;
pub mod config_file;
pub mod payload;
pub mod server;
pub mod ui;

pub use payload::{ParseError, parse, project_key};
pub use server::{Received, Running, Settings, spawn, spawn_with_ui};
pub use ui::UiBackend;

/// A random 256-bit value as 64 hex characters, for tokens and salts.
///
/// # Errors
/// When the OS has no randomness source.
pub fn random_hex() -> Result<String, getrandom::Error> {
    use std::fmt::Write as _;
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    let mut out = String::with_capacity(64);
    for b in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}
