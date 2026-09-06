use crate::presence::{
    HEARTBEAT_EVERY_TICKS, LiveContext, Publish, db_activity, decide_publish, should_resend,
    static_activity,
};

fn no_live() -> LiveContext<'static> {
    LiveContext {
        uid_known: false,
        display_name: None,
        version: None,
    }
}

/// Shared rich fixture: Union 64 / America / 123 quests / v3.6.13.
fn rich_info() -> crate::db::PlayerInfo {
    crate::db::PlayerInfo {
        uid: "1".to_string(),
        union_level: "64".to_string(),
        region: "America".to_string(),
        quests_done: Some(123),
        game_version: Some("3.6.13".to_string()),
        display_name: None,
    }
}

#[test]
fn resend_on_first_send_and_on_change() {
    assert!(should_resend(None, r#"{"a":1}"#, 0));
    assert!(should_resend(Some(r#"{"a":1}"#), r#"{"a":2}"#, 11));
}

#[test]
fn identical_payload_skips_until_heartbeat() {
    let body = r#"{"a":1}"#;
    for streak in 0..HEARTBEAT_EVERY_TICKS {
        assert!(
            !should_resend(Some(body), body, streak),
            "streak {streak} must skip"
        );
    }
    assert!(should_resend(Some(body), body, HEARTBEAT_EVERY_TICKS));
}

#[test]
fn decide_publish_prefers_rich_then_flag_then_silence() {
    let info = crate::db::PlayerInfo {
        uid: "1".to_string(),
        union_level: "60".to_string(),
        region: "Europe".to_string(),
        quests_done: None,
        game_version: None,
        display_name: None,
    };

    // Rich data always publishes, regardless of the flag.
    for flag in [false, true] {
        let activity = decide_publish(Some(&info), flag, 0, None, None, no_live()).into_activity();
        assert_eq!(activity, Some(db_activity(0, &info, None, None)));
    }

    // A display name takes over details; absence keeps the level line.
    let activity =
        decide_publish(Some(&info), false, 0, Some("Rover"), None, no_live()).into_activity();
    assert_eq!(activity, Some(db_activity(0, &info, Some("Rover"), None)));
    assert_eq!(activity.unwrap()["details"], "Rover");

    // Without data: static text only under the flag, else silence.
    let activity = decide_publish(None, true, 0, None, None, no_live()).into_activity();
    assert_eq!(activity, Some(static_activity(0)));
    assert!(matches!(
        decide_publish(None, false, 0, None, None, no_live()),
        Publish::Silent
    ));
    assert_eq!(
        decide_publish(None, false, 0, None, None, no_live()).into_activity(),
        None
    );
}

#[test]
fn decide_publish_complements_sources_without_guessing() {
    let live = LiveContext {
        uid_known: true,
        display_name: Some("Rover"),
        version: Some("3.6.13"),
    };
    // Identity + name, no level data: named static, never silence, never a guess.
    let activity = decide_publish(None, false, 0, None, None, live).into_activity();
    let activity = activity.expect("named static publishes");
    assert_eq!(activity["details"], "Rover");
    assert_eq!(activity["state"], "Exploring SOL-III");
    assert_eq!(activity["assets"]["small_text"], "v3.6.13");

    // Identity without a name: still silent (a bare generic line would add
    // nothing over game detection).
    let nameless = LiveContext {
        display_name: None,
        ..live
    };
    assert!(matches!(
        decide_publish(None, false, 0, None, None, nameless),
        Publish::Silent
    ));

    // No identity at all: silent even with leftovers present.
    assert!(matches!(
        decide_publish(None, false, 0, None, None, no_live()),
        Publish::Silent
    ));
}

#[test]
fn db_activity_layout_is_name_then_level_pipe_region() {
    let info = rich_info();

    let value = db_activity(0, &info, Some("TestRover"), None);
    assert_eq!(value["details"], "TestRover");
    assert_eq!(value["state"], "Lv. 64 | Region: America");
    assert_eq!(value["assets"]["small_text"], "123 quests • v3.6.13");

    // No name, no quests, no version: level line, region-only state, no small.
    let bare = crate::db::PlayerInfo {
        quests_done: None,
        game_version: None,
        ..info.clone()
    };
    let value = db_activity(0, &bare, None, None);
    assert_eq!(value["details"], "Union Level 64");
    assert_eq!(value["state"], "Lv. 64 | Region: America");
    assert!(value["assets"].get("small_image").is_none());
}

#[test]
fn character_icon_owns_small_slot_with_name_first() {
    use crate::character::find;

    let denia = find("denia").unwrap();
    let info = rich_info();
    let value = db_activity(0, &info, Some("TestRover"), Some(&denia));
    assert_eq!(value["details"], "TestRover");
    assert_eq!(value["assets"]["small_image"], "denia");
    assert_eq!(
        value["assets"]["small_text"],
        "Denia • 123 quests • v3.6.13"
    );

    // Rich decision threads the character through.
    let activity = decide_publish(Some(&info), false, 0, None, Some(&denia), no_live())
        .into_activity()
        .unwrap();
    assert_eq!(activity["assets"]["small_image"], "denia");

    // Named-static honors it too; unknown names never reach here (main
    // warns and passes None), so no fallback case exists at this layer.
    let live = LiveContext {
        uid_known: true,
        display_name: Some("Rover"),
        version: Some("3.6.13"),
    };
    let activity = decide_publish(None, false, 0, None, Some(&denia), live)
        .into_activity()
        .unwrap();
    assert_eq!(activity["assets"]["small_image"], "denia");
    assert_eq!(activity["assets"]["small_text"], "Denia • v3.6.13");
}
