use crate::detect::{
    Detector, WUWA_ID, load_cache, process_start_millis, select_wuwa, store_cache,
};
use crate::presence::now_millis;

#[test]
fn wuwa_id_matches_steam_appid() {
    assert_eq!(WUWA_ID, "1247227126416146462");
}

#[test]
fn detect_returns_without_error() {
    // Read-only /proc scan; the result depends on whether the game runs,
    // but the call itself must never fail.
    let detector = Detector::new().expect("detector builds");
    let found = detector.detect().expect("scan works");
    if let Some(game) = found {
        assert_eq!(game.id, WUWA_ID);
    }
}

#[test]
fn own_process_start_time_is_sane() {
    let pid = u64::from(std::process::id());
    let start = process_start_millis(pid).expect("own start time is readable");
    let now = now_millis();
    assert!(
        start > 0 && start <= now,
        "start {start} must be within (0, {now}]"
    );
}

#[test]
fn unknown_pid_has_no_start_time() {
    assert_eq!(process_start_millis(2147483647), None);
}

#[test]
fn select_wuwa_keeps_entry_and_merges_linux_override() {
    let online = r#"[
    {"id": "1247227126416146462", "name": "Wuthering Waves", "hook": false,
     "executables": [{"name": "wuthering waves.exe", "is_launcher": false, "os": "win32"}]},
    {"id": "999", "name": "Other", "hook": false}
  ]"#;
    let merged = select_wuwa(online).expect("entry present");
    let games: Vec<serde_json::Value> = serde_json::from_str(&merged).unwrap();
    assert_eq!(games.len(), 2, "online entry plus local override");
    assert!(games.iter().all(|g| g["id"] == WUWA_ID));
    let override_exe = &games[1]["executables"];
    assert_eq!(override_exe[0]["name"], "client-win64-shipping.exe");
    assert_eq!(override_exe[0]["os"], "linux");
}

#[test]
fn select_wuwa_rejects_databases_without_the_game() {
    assert_eq!(select_wuwa("[]"), None);
    assert_eq!(select_wuwa("not json"), None);
}

fn cache_selection() -> String {
    format!(r#"[{{"id": "{WUWA_ID}", "name": "Wuthering Waves", "hook": false}}]"#)
}

fn unique_cache_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wwrpc-cache-{}-{tag}", std::process::id()))
}

#[test]
fn cache_roundtrip_serves_fresh_and_rejects_garbage() {
    let dir = unique_cache_dir("roundtrip");
    let _ = std::fs::remove_dir_all(&dir);

    // Missing cache: nothing to serve.
    assert_eq!(load_cache(&dir, true), None);

    store_cache(&dir, &cache_selection());
    assert_eq!(load_cache(&dir, true), Some(cache_selection()));

    // Garbage and wrong-id caches are ignored, never fatal.
    std::fs::write(dir.join("detectable.json"), "not json").unwrap();
    assert_eq!(load_cache(&dir, true), None);
    assert_eq!(load_cache(&dir, false), None);
    std::fs::write(dir.join("detectable.json"), r#"[{"id": "999"}]"#).unwrap();
    assert_eq!(load_cache(&dir, true), None);

    let _ = std::fs::remove_dir_all(&dir);
}
