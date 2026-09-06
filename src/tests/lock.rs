use crate::lock::SingleInstance;

fn unique_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wwrpc-lock-{}-{}", std::process::id(), tag))
}

#[test]
fn second_acquire_fails_while_first_is_held() {
    let dir = unique_dir("held");
    let _ = std::fs::remove_dir_all(&dir);
    let _first = SingleInstance::acquire(&dir).expect("first acquire works");
    assert!(
        SingleInstance::acquire(&dir).is_err(),
        "a live instance must block a second one"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_lock_files_dont_block_startup() {
    let dir = unique_dir("stale");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Unparsable content.
    std::fs::write(dir.join("wwrpc.pid"), "not-a-pid").unwrap();
    assert!(SingleInstance::acquire(&dir).is_ok());

    // Dead pid.
    std::fs::write(dir.join("wwrpc.pid"), "2147483647").unwrap();
    assert!(SingleInstance::acquire(&dir).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}
