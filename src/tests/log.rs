use crate::log::{configure, is_quiet, is_verbose, level_from_env};

fn with_env(vars: &[(&str, &str)], check: impl FnOnce()) {
    let saved: Vec<(String, Option<String>)> = vars
        .iter()
        .map(|(key, _)| (key.to_string(), std::env::var(key).ok()))
        .collect();
    for (key, value) in vars {
        // SAFETY: single-threaded test manipulation of the process env; the
        // values only steer log-level mapping, never memory.
        unsafe { std::env::set_var(key, value) };
    }
    check();
    for (key, previous) in saved {
        // SAFETY: restoring exactly what this helper changed, same scope.
        unsafe {
            match previous {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }
    }
    configure(false, false);
}

#[test]
fn log_level_env_mapping() {
    // Pure mapping first: no globals involved.
    with_env(&[("WWRPC_LOG", "debug")], || {
        assert_eq!(level_from_env(), (true, false));
    });
    with_env(&[("WWRPC_LOG", "QUIET")], || {
        assert_eq!(level_from_env(), (false, true));
    });
    with_env(&[("WWRPC_VERBOSE", "yes")], || {
        assert_eq!(level_from_env(), (true, false));
    });
    with_env(&[("WWRPC_QUIET", "1")], || {
        assert_eq!(level_from_env(), (false, true));
    });
    with_env(&[("WWRPC_LOG", "info")], || {
        assert_eq!(level_from_env(), (false, false));
    });

    // Flags OR env through the globals, then restored to flag-only.
    with_env(&[("WWRPC_LOG", "debug")], || {
        configure(false, false);
        assert!(is_verbose());
        assert!(!is_quiet());
    });
    assert!(!is_verbose());
}
