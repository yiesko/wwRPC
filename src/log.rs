// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! Timestamped stderr logging with levels. `info`/`debug` go quiet with
//! `--quiet`; `debug` additionally needs `--verbose`. Warnings and errors
//! always print: daemons are debugged through these lines.
//!
//! Environment (checked on every [`configure`] call; either flag or env
//! enables — quiet wins when both ask):
//! - `WWRPC_LOG`: `debug`/`trace` (verbose on), `quiet`/`off`/`none`
//!   (quiet on), anything else (flags only).
//! - `WWRPC_VERBOSE=1` / `WWRPC_QUIET=1` (also accept
//!   `true`/`yes`/`on`, case-insensitive).

use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);

/// Apply `--verbose` / `--quiet` once at startup, OR-ed with the
/// environment (see module docs). Calling again re-reads the env, which
/// is what makes the level observable in tests.
pub fn configure(verbose: bool, quiet: bool) {
    let (env_verbose, env_quiet) = level_from_env();
    VERBOSE.store(verbose || env_verbose, Ordering::Relaxed);
    QUIET.store(quiet || env_quiet, Ordering::Relaxed);
}

/// `(verbose, quiet)` requested by the environment alone. Pure over the
/// process env so the mapping stays testable without touching globals.
pub(crate) fn level_from_env() -> (bool, bool) {
    fn truthy(name: &str) -> bool {
        std::env::var(name)
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }
    let mut verbose = truthy("WWRPC_VERBOSE");
    let mut quiet = truthy("WWRPC_QUIET");
    if let Ok(level) = std::env::var("WWRPC_LOG") {
        match level.trim().to_ascii_lowercase().as_str() {
            "debug" | "trace" => verbose = true,
            "quiet" | "off" | "none" | "silent" => quiet = true,
            _ => {}
        }
    }
    (verbose, quiet)
}

/// Whether `debug` lines currently print (flags or env).
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed) && !QUIET.load(Ordering::Relaxed)
}

/// Whether `info`/`debug` lines are currently silenced (flags or env).
pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

fn timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn emit(level: &str, message: &str) {
    eprintln!("[{} {level}] {message}", timestamp());
}

/// Routine state changes. Silenced by `--quiet`.
pub fn info(message: impl AsRef<str>) {
    if !QUIET.load(Ordering::Relaxed) {
        emit("INFO", message.as_ref());
    }
}

/// Transient problems that degrade gracefully. Always shown.
pub fn warn(message: impl AsRef<str>) {
    emit("WARN", message.as_ref());
}

/// Failures. Always shown.
pub fn error(message: impl AsRef<str>) {
    emit("ERROR", message.as_ref());
}

/// Per-tick internals (refresh outcomes, send/skip decisions). Needs
/// `--verbose` (and not `--quiet`).
pub fn debug(message: impl AsRef<str>) {
    if VERBOSE.load(Ordering::Relaxed) && !QUIET.load(Ordering::Relaxed) {
        emit("DEBUG", message.as_ref());
    }
}
