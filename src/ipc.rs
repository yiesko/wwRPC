// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! Minimal Discord IPC client (Unix socket): handshake, `SET_ACTIVITY`
//! and clear. Only what wwrpc needs: no subscriptions, no proxying.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
/// Largest acceptable reply: Discord frames are kilobytes; anything past
/// this is a desync, and must not become an allocation.
const MAX_REPLY_BYTES: u32 = 1024 * 1024;
/// Sockets probed per base directory (`discord-ipc-0..9`).
const MAX_IPC_SOCKETS: u8 = 10;
/// Give up on slow/dead sockets instead of hanging a tick.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// One Discord IPC frame: `op` + little-endian length + JSON body.
pub(crate) fn encode_frame(op: u32, payload: &str) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload.as_bytes());
    frame
}

/// Candidate Discord sockets: `discord-ipc-0..9` under `$XDG_RUNTIME_DIR`
/// (native) and the system temp dir (Flatpak/Snap re-exports), in order.
pub(crate) fn socket_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let bases = [
        std::env::var("XDG_RUNTIME_DIR").ok().map(PathBuf::from),
        Some(std::env::temp_dir()),
    ];
    for base in bases.into_iter().flatten() {
        for n in 0..MAX_IPC_SOCKETS {
            paths.push(base.join(format!("discord-ipc-{n}")));
        }
    }
    // `$XDG_RUNTIME_DIR/discord-ipc-N` covers native installs; the `/tmp`
    // entries cover Flatpak/Snap layouts that re-export sockets there.
    paths
}

/// Minimal Discord client: one Unix socket, handshake on `connect`,
/// `SET_ACTIVITY` per tick. Reconnect by dropping and re-`connect`ing;
/// reply reads are best-effort (a missed reply heals on the next tick).
pub struct IpcClient {
    client_id: String,
    stream: Option<UnixStream>,
    nonce: u64,
}

impl IpcClient {
    /// Unconnected client for one Discord application id. No I/O happens
    /// until [`IpcClient::connect`].
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            stream: None,
            nonce: 0,
        }
    }

    /// `true` after a successful `connect`, until `close` or a write error.
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Connect to Discord (tries `discord-ipc-0..9`) and handshake.
    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for path in socket_paths() {
            if let Ok(stream) = UnixStream::connect(&path) {
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                self.stream = Some(stream);
                self.send(
                    OP_HANDSHAKE,
                    &json!({"v": 1, "client_id": self.client_id}).to_string(),
                )?;
                // Tolerate a missing/slow handshake reply; the next failed write
                // will surface a dead connection anyway.
                let _ = self.read_reply();
                return Ok(());
            }
        }
        Err("no Discord IPC socket found (is Discord running?)".into())
    }

    /// Publish an activity for `pid`. Read errors on the reply are ignored:
    /// presence is re-sent every tick, so a dropped reply is harmless, while
    /// write errors are returned to trigger a reconnect.
    pub fn set_activity(
        &mut self,
        pid: u64,
        activity: Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.nonce += 1;
        let payload = json!({
          "cmd": "SET_ACTIVITY",
          "args": {"pid": pid, "activity": activity},
          "nonce": self.nonce.to_string(),
        });
        self.send(OP_FRAME, &payload.to_string())?;
        let _ = self.read_reply();
        Ok(())
    }

    /// Best-effort clear; never fails the caller.
    pub fn clear(&mut self, pid: u64) {
        self.nonce += 1;
        let payload = json!({
          "cmd": "SET_ACTIVITY",
          "args": {"pid": pid, "activity": Value::Null},
          "nonce": self.nonce.to_string(),
        });
        if let Ok(body) = serde_json::to_string(&payload) {
            let _ = self.send(OP_FRAME, &body);
            let _ = self.read_reply();
        }
    }

    /// Drop the socket without a goodbye frame; the next `connect` starts
    /// over. Idempotent.
    pub fn close(&mut self) {
        self.stream = None;
    }

    fn send(&mut self, op: u32, payload: &str) -> std::io::Result<()> {
        match self.stream.as_mut() {
            Some(stream) => {
                stream.write_all(&encode_frame(op, payload))?;
                stream.flush()
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC not connected",
            )),
        }
    }

    fn read_reply(&mut self) -> std::io::Result<Value> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC not connected",
            )
        })?;
        let mut header = [0u8; 8];
        stream.read_exact(&mut header)?;
        let len = u32::from_le_bytes(header[4..8].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "short IPC header")
        })?);
        if len > MAX_REPLY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "IPC reply exceeds sanity size",
            ));
        }
        let mut body = vec![0u8; len as usize];
        stream.read_exact(&mut body)?;
        serde_json::from_slice(&body)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
