// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! Presence payload builders. Pure data, no I/O: easy to test.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::character::Character;
use crate::db::PlayerInfo;

/// Ticks with byte-identical payload between forced resends. Skipping
/// duplicates saves IPC traffic, but a heartbeat keeps a silently dead
/// connection from going unnoticed forever.
pub const HEARTBEAT_EVERY_TICKS: u32 = 12;

/// What a tick decided to do. `Silent` means: clear once (if anything of
/// ours is showing) and stay quiet, so game detection (e.g. rsRPC) owns
/// the slot instead of being shadowed by a static fallback.
pub enum Publish {
    Rich(Value),
    NamedStatic(Value),
    Static(Value),
    Silent,
}

impl Publish {
    /// The wire payload, or `None` for `Silent` (caller clears once, then
    /// stays quiet so game detection owns the slot).
    pub fn into_activity(self) -> Option<Value> {
        match self {
            Self::Rich(activity) | Self::NamedStatic(activity) | Self::Static(activity) => {
                Some(activity)
            }
            Self::Silent => None,
        }
    }
}

/// Best-effort live session context, independent of triage: every source
/// complements the others, so the decision takes what each one could
/// prove on this tick.
#[derive(Clone, Copy)]
pub struct LiveContext<'a> {
    /// Login uid readable right now, if any.
    pub uid_known: bool,
    /// Display name for that login (explicit flag already applied by the
    /// caller when set), if any.
    pub display_name: Option<&'a str>,
    /// Game version as of the latest successful device read, if any.
    pub version: Option<&'a str>,
}

/// Rich data wins; without it, a named identity still publishes (it is
/// verified live data, not a guess); without that, static text only under
/// `--static-fallback`, otherwise silence (clear + nothing).
pub fn decide_publish(
    info: Option<&PlayerInfo>,
    static_fallback: bool,
    start_ms: i64,
    player_name: Option<&str>,
    character: Option<&Character>,
    live: LiveContext<'_>,
) -> Publish {
    match (info, static_fallback) {
        (Some(info), _) => Publish::Rich(db_activity(start_ms, info, player_name, character)),
        (None, _) if live.uid_known => match player_name.or(live.display_name) {
            Some(name) => Publish::NamedStatic(named_static_activity(
                start_ms,
                name,
                live.version,
                character,
            )),
            None => Publish::Silent,
        },
        (None, true) => Publish::Static(static_activity(start_ms)),
        (None, false) => Publish::Silent,
    }
}
/// Whether play moved from one known account to another, in which case
/// the session clock must restart. `None` on either side means "unknown",
/// never a switch: data gaps must not reset the clock.
///
/// # Examples
///
/// ```
/// use wwrpc::presence::account_switched;
///
/// assert!(account_switched(Some("1"), Some("2")));
/// assert!(!account_switched(Some("1"), Some("1")));
/// assert!(!account_switched(Some("1"), None));
/// assert!(!account_switched(None, Some("2")));
/// assert!(!account_switched(None, None));
/// ```
pub fn account_switched(previous: Option<&str>, current: Option<&str>) -> bool {
    matches!((previous, current), (Some(a), Some(b)) if a != b)
}

/// Whether to (re)send `body`: always on first send or content change,
/// otherwise only as a periodic heartbeat. `unchanged_streak` counts
/// consecutive skipped ticks; the caller resets it on every send.
///
/// # Examples
///
/// ```
/// use wwrpc::presence::{HEARTBEAT_EVERY_TICKS, should_resend};
///
/// assert!(should_resend(None, "{}", 0));
/// assert!(should_resend(Some("{}"), r#"{"a":1}"#, 0));
/// assert!(!should_resend(Some("{}"), "{}", 0));
/// assert!(should_resend(Some("{}"), "{}", HEARTBEAT_EVERY_TICKS));
/// ```
pub fn should_resend(last_sent: Option<&str>, body: &str, unchanged_streak: u32) -> bool {
    last_sent.is_none_or(|last| last != body) || unchanged_streak >= HEARTBEAT_EVERY_TICKS
}

/// Milliseconds since the Unix epoch, the unit Discord expects for
/// activity `timestamps.start`.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Companion presence for the official game app (in-Discord game page).
/// Mirrors the primary activity's details/state so both slots tell the
/// same story. With `portrait` (an external `https://` image URL), the
/// large slot shows that art — Discord proxies external URLs, so custom
/// art works here with zero uploads; without it, no image slot at all
/// (official-app keys are unknown, and a broken slot is worse than none).
pub fn official_activity(
    start_ms: i64,
    details: &str,
    state: &str,
    portrait: Option<&str>,
) -> Value {
    let mut activity = json!({
      "details": details,
      "timestamps": {"start": start_ms},
      "instance": false,
    });
    if !state.is_empty() {
        activity["state"] = json!(state);
    }
    if let Some(portrait) = portrait {
        activity["assets"] = json!({"large_image": portrait});
    }
    activity
}

/// Static presence: no game files involved at all.
pub fn static_activity(start_ms: i64) -> Value {
    json!({
      "details": "Exploring SOL-III",
      "assets": {"large_image": "logo", "large_text": "Wuthering Waves"},
      "timestamps": {"start": start_ms},
      "instance": false,
    })
}

/// Named static presence: verified live identity plus a display name, but
/// no level data. Every field shown is proven — the name matched this very
/// login, the version came from the device database — so this outranks
/// silence while staying below full rich data.
pub fn named_static_activity(
    start_ms: i64,
    name: &str,
    version: Option<&str>,
    character: Option<&Character>,
) -> Value {
    let mut activity = json!({
      "details": name,
      "state": "Exploring SOL-III",
      "assets": {"large_image": "logo", "large_text": "Wuthering Waves"},
      "timestamps": {"start": start_ms},
      "instance": false,
    });
    if let Some(character) = character {
        activity["assets"]["small_image"] = json!(character.asset);
        let mut bits = vec![character.display.to_string()];
        if let Some(version) = version {
            bits.push(format!("v{version}"));
        }
        activity["assets"]["small_text"] = json!(bits.join(" • "));
    } else if let Some(version) = version {
        activity["assets"]["small_image"] = json!("logo");
        activity["assets"]["small_text"] = json!(format!("v{version}"));
    }
    activity
}

/// Database presence layout:
/// - `details`: display name, or `Union Level {level}` without one.
/// - `state`: `Lv. {level} | Region: {region}`.
/// - `small_text` (hover): quest total and game version, when known.
pub fn db_activity(
    start_ms: i64,
    info: &PlayerInfo,
    player_name: Option<&str>,
    character: Option<&Character>,
) -> Value {
    let display = player_name.or(info.display_name.as_deref());
    let details = match display {
        Some(name) => name.to_string(),
        None => format!("Union Level {}", info.union_level),
    };
    let state = format!("Lv. {} | Region: {}", info.union_level, info.region);
    let mut activity = json!({
      "details": details,
      "state": state,
      "assets": {"large_image": "logo", "large_text": "Wuthering Waves"},
      "timestamps": {"start": start_ms},
      "instance": false,
    });
    let mut small_bits = Vec::new();
    if let Some(quests) = info.quests_done {
        small_bits.push(format!("{quests} quests"));
    }
    if let Some(version) = info.game_version.as_deref() {
        small_bits.push(format!("v{version}"));
    }
    if let Some(character) = character {
        // Character icon owns the small slot: its name leads, quest and
        // version bits ride along. Without one, the legacy logo+bits rule.
        activity["assets"]["small_image"] = json!(character.asset);
        let mut bits = vec![character.display.to_string()];
        bits.extend(small_bits);
        activity["assets"]["small_text"] = json!(bits.join(" • "));
    } else if !small_bits.is_empty() {
        activity["assets"]["small_image"] = json!("logo");
        activity["assets"]["small_text"] = json!(small_bits.join(" • "));
    }
    activity
}
