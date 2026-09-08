// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! wwrpc binary: wait for Wuthering Waves, publish Rich Presence.
//!
//! Two polling tiers keep it light: a fast detection tick (default 5s,
//! `/proc` scan only) and a slower presence tick (default 15s: snapshot,
//! parse, send). Identical payloads are not resent except as a periodic
//! heartbeat. Default mode reads Union Level + Region from a RAM copy of
//! `LocalStorage.db` (the live game files are never SQLite-opened,
//! written or chmodded). `--no-database` disables all game-file access.

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use clap::Parser;
use wwrpc::db::DbReader;
use wwrpc::detect::{Detector, process_start_millis};
use wwrpc::ipc::IpcClient;
use wwrpc::lock::SingleInstance;
use wwrpc::log;
use wwrpc::presence::{LiveContext, account_switched, now_millis, should_resend};

/// Default Discord application id: the community app carrying the logo
/// and portrait assets (see `# Character icons` in the README). Override
/// with `--app-id` / `WWRPC_APP_ID` for a personal app.
const DEFAULT_APP_ID: &str = "1546176429048463360";

/// Official Wuthering Waves application: publishing here links the
/// presence to the in-Discord game page (directory entry). Custom art
/// lives on the user's own app and will NOT render in this slot.
const OFFICIAL_APP_ID: &str = "1247227126416146462";

/// Resolve the character icon: `--character` wins, `WWRPC_CHARACTER`
/// fills in when the flag is absent; blank means unset. Unknown names
/// warn (with the input echoed — it is the operator's own flag, not game
/// data) and publish without an icon instead of failing.
fn resolve_character(input: Option<&str>) -> Option<wwrpc::character::Character> {
    let input = input.map(str::trim).filter(|input| !input.is_empty())?;
    match wwrpc::character::find(input) {
        Some(character) => Some(character),
        None => {
            log::warn(format!(
                "unknown character '{input}'; run --list-characters for valid names, publishing without an icon"
            ));
            None
        }
    }
}

/// `--character` wins, `WWRPC_CHARACTER` fills in when the flag is
/// absent; blank means unset.
fn character_input(flag: Option<&str>) -> Option<String> {
    flag.map(str::trim)
        .filter(|flag| !flag.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("WWRPC_CHARACTER")
                .ok()
                .map(|env| env.trim().to_string())
                .filter(|env| !env.is_empty())
        })
}

/// Dedicated runtime directory. `$XDG_RUNTIME_DIR` is tmpfs by spec, so
/// snapshots, journal copies and the instance lock all live in RAM and
/// vanish on reboot/logout. Falls back to the system temp dir.
fn snapshot_dir() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("wwrpc")
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Wuthering Waves Discord Rich Presence for Linux"
)]
struct Args {
    /// Game install directory (auto-discovered when omitted: Steam,
    /// ~/Games, Twintail data; standalone layouts included).
    #[arg(long)]
    game_dir: Option<String>,

    /// Disable all game-file access; show static presence instead.
    #[arg(long)]
    no_database: bool,

    /// Publish static "Exploring SOL-III" text when there is no rich data.
    /// Default (off): stay silent so game detection (e.g. rsRPC) owns the
    /// slot instead of being shadowed by a static fallback.
    #[arg(long)]
    static_fallback: bool,

    /// Kuro Games UID, matched against the level database. Defaults to the
    /// auto-detected RecentlyLoginUID. Prefer `--account-index`: uids differ
    /// per login space and yours may simply have no level row.
    #[arg(long)]
    kuro_uid: Option<String>,

    /// Use the Nth stored account (see `--list-accounts`) instead of uid
    /// resolution. Wins over `--kuro-uid`.
    #[arg(long)]
    account_index: Option<usize>,

    /// Display name shown as `{name} • Union Level {level}` in details.
    /// Free text (your Rover name); never read from game files.
    #[arg(long)]
    player_name: Option<String>,

    /// Character icon (`small_image`): any name from `--list-characters`
    /// (e.g. `--character Cartethyia`). Needs the matching art asset
    /// uploaded to the Discord app under the same lowercase name; unknown
    /// names warn and publish without an icon. Also `WWRPC_CHARACTER`.
    #[arg(long)]
    character: Option<String>,

    /// Companion presence on the official game app for the in-Discord game
    /// page (same details/state, no custom art). Needs rsRPC (or anything
    /// else) to NOT publish that slot: set `--ignore-ids` there. Also
    /// `WWRPC_GAME_PAGE=1`.
    #[arg(long)]
    game_page: bool,

    /// Companion slot shows the character portrait via external URL
    /// (zero uploads, renders anywhere). Needs `--game-page` and
    /// `--character`. Also `WWRPC_GAME_PAGE_PORTRAITS=1`.
    #[arg(long)]
    game_page_portraits: bool,

    /// List selectable character names (`Display (key)`) and exit.
    #[arg(long)]
    list_characters: bool,

    /// List stored accounts (`N: Region, Union LEVEL`) and exit. Uids are
    /// never printed: recognize yours by level.
    #[arg(long)]
    list_accounts: bool,

    /// Print the exact activity JSON that would be published right now and
    /// exit. Dry run: no IPC, no lock, snapshot in an isolated temp dir.
    #[arg(long)]
    print_activity: bool,

    /// Seconds between presence updates (minimum 5).
    #[arg(long, default_value_t = 15)]
    interval: u64,

    /// Seconds between game-detection scans (minimum 1).
    #[arg(long, default_value_t = 5)]
    detect_interval: u64,

    /// Discord application id. Custom icons need YOUR OWN app (see
    /// `# Character icons` in the README): create one, upload the art, and
    /// point here via `--app-id` or `WWRPC_APP_ID` (flag wins, blank is unset).
    #[arg(long, default_value = DEFAULT_APP_ID)]
    app_id: String,

    /// Verbose per-tick debug logging.
    #[arg(long, conflicts_with = "quiet")]
    verbose: bool,

    /// Only warnings and errors.
    #[arg(long)]
    quiet: bool,
}

/// One game run: pid, session clock, and what was last published.
#[derive(Debug)]
struct Session {
    pid: u64,
    start_ms: i64,
    /// Account uid behind the current data, to restart the clock on switch.
    uid: Option<String>,
    last_body: Option<String>,
    unchanged_streak: u32,
    last_send: Option<Instant>,
    /// A clear was already sent for the current silent stretch.
    silence_cleared: bool,
    /// Last publish tier, to announce tier changes once (INFO).
    last_tier: Option<&'static str>,
}

impl Session {
    fn new(pid: u64) -> Self {
        // Anchor the clock to the real launch when readable; the tick we
        // noticed it is only the fallback.
        let start_ms = process_start_millis(pid).unwrap_or_else(now_millis);
        Self {
            pid,
            start_ms,
            uid: None,
            last_body: None,
            unchanged_streak: 0,
            last_send: None,
            silence_cleared: false,
            last_tier: None,
        }
    }
}

/// Truthy env (`1/true/yes/on`, any case): `--flag` wins, env fills in.
fn env_flag(flag: bool, name: &str) -> bool {
    flag || std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Short tier name for logging: what a tick decided, in one word.
fn tier_name(publish: &wwrpc::presence::Publish) -> &'static str {
    match publish {
        wwrpc::presence::Publish::Rich(_) => "rich",
        wwrpc::presence::Publish::NamedStatic(_) => "named-static",
        wwrpc::presence::Publish::Static(_) => "static",
        wwrpc::presence::Publish::Silent => "silent",
    }
}

/// Sleep slice: Ctrl+C stays responsive without busy-waiting.
const SLEEP_SLICE: Duration = Duration::from_millis(250);

/// Sleep in small slices so Ctrl+C stays responsive.
fn sleep_checked(total: Duration, running: &AtomicBool) {
    let mut waited = Duration::ZERO;
    while running.load(Ordering::SeqCst) && waited < total {
        std::thread::sleep(SLEEP_SLICE.min(total - waited));
        waited += SLEEP_SLICE;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    log::configure(args.verbose, args.quiet);

    let running = Arc::new(AtomicBool::new(true));
    ctrlc::set_handler({
        let running = running.clone();
        move || running.store(false, Ordering::SeqCst)
    })?;

    // Fail fast on a second copy instead of double-publishing presence.
    // (Diagnostics like --list-accounts run before this on purpose.)
    let snapshots = snapshot_dir();

    let game_dir = args.game_dir.as_deref().map(std::path::Path::new);
    let (game_dir, storage_dir) =
        wwrpc::db::resolve_storage(game_dir, &wwrpc::db::default_roots())?;

    if args.list_accounts {
        let accounts = DbReader::list_accounts(&storage_dir, &game_dir)?;
        if accounts.is_empty() {
            println!("No stored accounts with level data.");
        } else {
            for account in &accounts {
                let marker = if account.last_login {
                    " [last launcher login]"
                } else {
                    ""
                };
                match account.nickname.as_deref() {
                    // Login nickname: recognition aid, may be outdated — never used
                    // for matching or display.
                    Some(nickname) => println!(
                        "{}: {}, Union {} (login nickname: {nickname}?){marker}",
                        account.index, account.region, account.level
                    ),
                    None => println!(
                        "{}: {}, Union {}{marker}",
                        account.index, account.region, account.level
                    ),
                }
            }
            println!("Nicknames in () are login nicknames and may be outdated;");
            println!("recognize yours by region/level, then pin it with --account-index N.");
        }
        return Ok(());
    }

    if args.list_characters {
        for character in wwrpc::character::all() {
            println!("{} ({})", character.display, character.key);
        }
        println!("Upload each portrait to the Discord app under its lowercase key,");
        println!("then select it with --character <name>.");
        return Ok(());
    }

    let character = resolve_character(character_input(args.character.as_deref()).as_deref());
    let game_page = env_flag(args.game_page, "WWRPC_GAME_PAGE");
    let game_page_portraits = env_flag(args.game_page_portraits, "WWRPC_GAME_PAGE_PORTRAITS");
    // Portrait URL for the companion slot (external art, zero uploads).
    // Needs both a character and the companion slot itself.
    let companion_portrait: Option<String> = if game_page_portraits {
        match (&character, game_page) {
            (Some(character), true) => Some(wwrpc::character::portrait_url(character)),
            (None, _) => {
                log::warn("--game-page-portraits needs --character; companion stays imageless");
                None
            }
            (_, false) => {
                log::warn("--game-page-portraits needs --game-page; ignoring");
                None
            }
        }
    } else {
        None
    };

    if args.print_activity {
        // Isolated temp snapshot: never touches the service's runtime dir.
        let diagnostics_dir =
            std::env::temp_dir().join(format!("wwrpc-print-{}", std::process::id()));
        let mut reader =
            DbReader::new(storage_dir, diagnostics_dir.clone(), args.kuro_uid.clone())?;
        reader.set_game_dir(game_dir.clone());
        if let Some(index) = args.account_index {
            reader.set_account_index(index);
        }
        let info = match reader.refresh() {
            wwrpc::db::Refresh::Updated | wwrpc::db::Refresh::Unchanged => reader.last_info(),
            wwrpc::db::Refresh::Unavailable => None,
        };
        let live = LiveContext {
            uid_known: reader.live_uid().is_some(),
            display_name: reader.display_name(),
            version: reader.cached_version(),
        };
        let publish = wwrpc::presence::decide_publish(
            info,
            args.static_fallback,
            now_millis(),
            args.player_name.as_deref(),
            character.as_ref(),
            live,
        );
        match publish.into_activity() {
            Some(activity) => {
                println!("{}", serde_json::to_string_pretty(&activity)?);
                if game_page
                    && let (Some(details), state) = (
                        activity.get("details").and_then(|v| v.as_str()),
                        activity.get("state").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                {
                    println!("--- official companion ---");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&wwrpc::presence::official_activity(
                            now_millis(),
                            details,
                            state,
                            companion_portrait.as_deref()
                        ))?
                    );
                }
            }
            None => println!("(silent: no rich data — game detection would own the slot)"),
        }
        let _ = std::fs::remove_dir_all(&diagnostics_dir);
        return Ok(());
    }

    let _instance = SingleInstance::acquire(&snapshots).map_err(|err| format!("[wwrpc] {err}"))?;

    let detector = Detector::new()?;
    let mut db = if args.no_database {
        None
    } else {
        if let Some(index) = args.account_index {
            // Advisory-only pre-validation: a torn startup copy must never
            // permanently disable the pin — per-tick triage retries anyway.
            match DbReader::list_accounts(&storage_dir, &game_dir) {
                Ok(accounts) if index == 0 || index > accounts.len() => {
                    log::warn(format!(
                        "account index {index} out of range ({} stored)",
                        accounts.len()
                    ));
                }
                Err(err) => {
                    log::warn(format!(
                        "account pre-validation skipped ({err}); retrying per tick"
                    ));
                }
                _ => {}
            }
        }
        let mut reader = DbReader::new(storage_dir, snapshots, args.kuro_uid.clone())?;
        reader.set_game_dir(game_dir.clone());
        if let Some(index) = args.account_index {
            reader.set_account_index(index);
        }
        Some(reader)
    };
    // `--app-id` wins, `WWRPC_APP_ID` fills in, default app otherwise.
    // (clap always fills the default, so "flag was passed" is detected by
    // comparing against it; blank env is unset.)
    let app_id = if args.app_id != DEFAULT_APP_ID {
        args.app_id.clone()
    } else {
        std::env::var("WWRPC_APP_ID")
            .ok()
            .map(|env| env.trim().to_string())
            .filter(|env| !env.is_empty())
            .unwrap_or_else(|| args.app_id.clone())
    };
    let mut ipc = IpcClient::new(app_id);
    // Companion slot on the official app (game page linkage). Separate
    // connection + session from the primary slot.
    let mut ipc_official = game_page.then(|| IpcClient::new(OFFICIAL_APP_ID.to_string()));
    // The companion MUST NOT reuse the game pid: the bridge keys presence
    // by socketId = pid, so two connections sharing one pid overwrite each
    // other and only the last-published slot displays. Our own pid is
    // stable for the session, nonzero (so clears still count as genuine),
    // and can never collide with the game pid.
    let official_pid = std::process::id() as u64;
    let presence_every = Duration::from_secs(args.interval.max(5));
    let detect_every = Duration::from_secs(args.detect_interval.max(1));

    let mut session: Option<Session> = None;
    let mut official_session: Option<Session> = None;
    let mut last_pid: Option<u64> = None;
    let mut announced_waiting_game = false;
    let mut announced_waiting_discord = false;
    let mut announced_waiting_discord_official = false;

    while running.load(Ordering::SeqCst) {
        let game = match detector.detect() {
            Ok(game) => game,
            Err(err) => {
                log::warn(format!("detection error: {err}"));
                sleep_checked(detect_every, &running);
                continue;
            }
        };

        let Some(game) = game else {
            if session.take().is_some() {
                log::info("game closed, clearing presence and waiting...");
                if let Some(pid) = last_pid {
                    ipc.clear(pid);
                    if let Some(official) = ipc_official.as_mut() {
                        official.clear(official_pid);
                    }
                }
                ipc.close();
                if let Some(official) = ipc_official.as_mut() {
                    official.close();
                }
                official_session = None;
            } else if !announced_waiting_game {
                log::info("waiting for Wuthering Waves...");
                announced_waiting_game = true;
            }
            sleep_checked(detect_every, &running);
            continue;
        };
        announced_waiting_game = false;
        // A missing pid should not happen (detection always stamps one);
        // skipping the tick beats publishing pid 0.
        let Some(pid) = game.pid else {
            log::warn("detected game without pid; skipping tick");
            sleep_checked(detect_every, &running);
            continue;
        };
        last_pid = Some(pid);

        // New pid (fresh launch or restart) starts a new session clock.
        let fresh = !matches!(&session, Some(active) if active.pid == pid);
        if fresh {
            if session.is_some() {
                log::info(format!("game restarted (pid {pid}), starting new session"));
            } else {
                log::info(format!("game detected (pid {pid}), starting session"));
            }
            announced_waiting_discord = false;
            session = Some(Session::new(pid));
            official_session = ipc_official.as_ref().map(|_| Session::new(pid));
        }
        let active = session.as_mut().expect("session just set");

        // Presence cadence runs on top of the faster detection tick.
        let due = active
            .last_send
            .is_none_or(|at| at.elapsed() >= presence_every);
        if !due {
            sleep_checked(detect_every, &running);
            continue;
        }

        if !ipc.is_connected() {
            match ipc.connect() {
                Ok(()) => {
                    log::info("connected to Discord");
                    announced_waiting_discord = false;
                }
                Err(err) => {
                    if !announced_waiting_discord {
                        log::warn(format!("{err}"));
                        announced_waiting_discord = true;
                    }
                    sleep_checked(detect_every, &running);
                    continue;
                }
            }
        }

        // Companion slot: same cadence, own connection. A failed companion
        // connect skips only this slot for the tick (never the primary).
        if let Some(official) = ipc_official.as_mut()
            && !official.is_connected()
        {
            match official.connect() {
                Ok(()) => {
                    log::info("connected to Discord (official slot)");
                    announced_waiting_discord_official = false;
                }
                Err(err) => {
                    if !announced_waiting_discord_official {
                        log::warn(format!("official slot: {err}"));
                        announced_waiting_discord_official = true;
                    }
                }
            }
        }

        let refresh = db.as_mut().map(|db| db.refresh());
        log::debug(format!(
            "presence tick: pid {pid}, refresh {:?}, degraded {}",
            refresh,
            db.as_ref().is_some_and(|db| db.is_degraded())
        ));
        // Stale-but-validated data survives transient blips; only a clean
        // triage state (or no data ever) decides what publishes.
        let info = db.as_ref().and_then(|db| db.last_info());
        // A different account behind the data restarts the session clock.
        // Unknown stretches (None) never count as a switch.
        let current_uid = info.map(|info| info.uid.as_str());
        if account_switched(active.uid.as_deref(), current_uid) {
            log::info("account switched; starting new session");
            active.start_ms = now_millis();
            active.uid = current_uid.map(str::to_string);
            active.last_body = None;
            active.unchanged_streak = 0;
            active.last_send = None;
            active.silence_cleared = false;
            // Companion slot restarts with it (same clock, fresh payload).
            if let Some(session) = official_session.as_mut() {
                session.start_ms = active.start_ms;
                session.uid = active.uid.clone();
                session.last_body = None;
                session.unchanged_streak = 0;
                session.last_send = None;
                session.silence_cleared = false;
            }
        } else if active.uid.is_none() {
            active.uid = current_uid.map(str::to_string);
        }
        // Every source complements the others: session identity (live login),
        // display name (explicit flag, else live-resolved), version (cached).
        let live = LiveContext {
            uid_known: db.as_ref().and_then(|db| db.live_uid()).is_some(),
            display_name: db.as_ref().and_then(|db| db.display_name()),
            version: db.as_ref().and_then(|db| db.cached_version()),
        };
        let publish = wwrpc::presence::decide_publish(
            info,
            args.static_fallback,
            active.start_ms,
            args.player_name.as_deref(),
            character.as_ref(),
            live,
        );
        // What the tick decided and from what: expected inputs (level data,
        // name, known login, static flag, character icon) next to the chosen
        // tier, so a silent stretch explains itself instead of just happening.
        let tier = tier_name(&publish);
        let named = args
            .player_name
            .as_deref()
            .or(live.display_name)
            .or(info.and_then(|info| info.display_name.as_deref()))
            .is_some();
        log::debug(format!(
            "publish tier: {tier} (level data: {}, name: {}, login known: {}, static flag: {}, character: {})",
            info.is_some(),
            named,
            live.uid_known,
            args.static_fallback,
            character.as_ref().map(|c| c.key).unwrap_or("none")
        ));
        if active.last_tier != Some(tier) {
            log::info(format!(
                "presence tier: {} -> {tier}",
                active.last_tier.unwrap_or("none")
            ));
            active.last_tier = Some(tier);
        }
        let Some(activity) = publish.into_activity() else {
            // Nothing rich to say: clear once and stay silent so game detection
            // (e.g. rsRPC) owns the slot instead of a static fallback.
            if !active.silence_cleared {
                ipc.clear(pid);
                if let Some(session) = official_session.as_mut() {
                    if let Some(official) = ipc_official.as_mut() {
                        official.clear(official_pid);
                    }
                    session.silence_cleared = true;
                }
                active.silence_cleared = true;
                log::info("no rich data; leaving presence to game detection");
            }
            sleep_checked(detect_every, &running);
            continue;
        };
        active.silence_cleared = false;
        // Companion slot follows the primary: same details/state (hence the
        // same tier decision), official app for the game-page linkage. No
        // custom art here by design — those keys only exist on our own app.
        if let (Some(session), Some(official)) = (official_session.as_mut(), ipc_official.as_mut())
        {
            session.silence_cleared = false;
            let details = activity
                .get("details")
                .and_then(|v| v.as_str())
                .unwrap_or("Wuthering Waves");
            let state = activity.get("state").and_then(|v| v.as_str()).unwrap_or("");
            let companion = wwrpc::presence::official_activity(
                active.start_ms,
                details,
                state,
                companion_portrait.as_deref(),
            );
            if let Ok(body) = serde_json::to_string(&companion) {
                if should_resend(
                    session.last_body.as_deref(),
                    &body,
                    session.unchanged_streak,
                ) {
                    match official.set_activity(official_pid, companion) {
                        Ok(()) => {
                            log::debug(format!("official slot sent ({} bytes)", body.len()));
                            session.last_body = Some(body);
                            session.unchanged_streak = 0;
                            session.last_send = Some(Instant::now());
                        }
                        Err(_) => {
                            log::warn("official slot: Discord connection lost, reconnecting...");
                            official.close();
                        }
                    }
                } else {
                    log::debug("official slot unchanged, skipping send");
                    session.unchanged_streak += 1;
                    session.last_send = Some(Instant::now());
                }
            }
        }
        let body = serde_json::to_string(&activity)?;
        if should_resend(active.last_body.as_deref(), &body, active.unchanged_streak) {
            match ipc.set_activity(pid, activity) {
                Ok(()) => {
                    log::debug(format!("presence sent ({} bytes)", body.len()));
                    active.last_body = Some(body);
                    active.unchanged_streak = 0;
                    active.last_send = Some(Instant::now());
                    active.silence_cleared = false;
                }
                Err(_) => {
                    log::warn("Discord connection lost, reconnecting...");
                    ipc.close();
                }
            }
        } else {
            log::debug("presence unchanged, skipping send");
            active.unchanged_streak += 1;
            active.last_send = Some(Instant::now());
        }

        sleep_checked(detect_every, &running);
    }

    if let Some(pid) = last_pid {
        ipc.clear(pid);
        if let Some(official) = ipc_official.as_mut() {
            official.clear(official_pid);
        }
    }
    Ok(())
}
