// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! Single-instance guard via PID file: refuses to start a second copy,
//! so two instances never double-publish presence or duplicate snapshots.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SingleInstance {
    path: PathBuf,
}

impl SingleInstance {
    /// Claim `dir/wwrpc.pid`. Errors when another live `wwrpc` holds it.
    /// Stale files (dead pid, unparsable content, or a recycled pid now
    /// owned by another program) are replaced instead of blocking startup.
    pub fn acquire(dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = dir.join("wwrpc.pid");
        if let Ok(previous) = fs::read_to_string(&path)
            && let Ok(pid) = previous.trim().parse::<u32>()
            && pid_alive_by_us(pid)
        {
            return Err(format!("another wwrpc instance is running (pid {pid})").into());
        }
        fs::create_dir_all(dir)
            .map_err(|err| format!("cannot create runtime dir '{}': {err}", dir.display()))?;
        fs::write(&path, std::process::id().to_string())
            .map_err(|err| format!("cannot write lock file '{}': {err}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // Best effort: a crash leaves the file behind, but the aliveness
        // check above keeps a stale file from blocking the next start.
        let _ = fs::remove_file(&self.path);
    }
}

/// `true` when `pid` exists AND its own executable (argv[0]) is wwrpc.
/// Only argv[0] counts: matching the whole command line would false-positive
/// on e.g. an editor with a wwrpc path open.
fn pid_alive_by_us(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .ok()
        .and_then(|cmdline| cmdline.split('\0').next().map(str::to_string))
        .and_then(|argv0| {
            Path::new(&argv0)
                .file_name()
                .map(|name| name.to_string_lossy().contains("wwrpc"))
        })
        .unwrap_or(false)
}
