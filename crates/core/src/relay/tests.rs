use super::*;
use serde_json::json;

fn report(action: &str) -> Value {
    json!({"command":"pull","ok":true,"data":{"game":"uncharted-2-slot-3","action":action}})
}

#[test]
fn distinguishes_offline_deferred_retry_and_nonzero_conflicts() {
    let read = |phase, code, value: Value| {
        decode(phase, Game::U2, 3, Some(code), value.to_string().as_bytes())
    };
    assert_eq!(read("pull", 0, report("offline")).unwrap(), Sync::Offline);
    let mut push = report("deferred");
    push["command"] = "push".into();
    assert_eq!(read("push", 0, push).unwrap(), Sync::Offline);
    // After offline progress rc.2 can upload during the next pull.
    assert_eq!(
        read("pull", 0, report("pushed")).unwrap(),
        Sync::Done("uploaded saves".into())
    );
    let mut conflict = report("conflict");
    conflict["data"]["server"] =
        json!({"device":"tablet","time":"2026-11-17T00:00:00Z","revision":7});
    conflict["data"]["local"] =
        json!({"device":"desktop","time":"2026-11-16T00:00:00Z","revision":null});
    conflict["data"]["server_card"] = json!({"place":"Chapter two","playtime_secs":40});
    assert!(read("pull", 0, conflict.clone()).is_err());
    let Sync::Conflict(cards) = read("pull", 3, conflict).unwrap() else {
        panic!("missing choice")
    };
    assert!(cards.server_is_newer);
    assert_eq!(cards.server.slot, "Revision 7");
    assert_eq!(cards.server.detail.as_deref(), Some("Chapter two"));
    assert_eq!(cards.server.play_time_secs, Some(40));
    assert_eq!(cards.local.device.as_deref(), Some("desktop"));
}

#[test]
fn failures_never_become_success_or_leak_provider_output() {
    assert!(
        decode("pull", Game::U2, 3, Some(0xc0000135_u32 as i32), b"")
            .unwrap_err()
            .to_string()
            .contains("Visual C++ x64")
    );
    for value in [
        report("unknown"),
        report("held"),
        json!({"ok":true,"command":"push"}),
        json!({"ok":true,"command":"pull","data":{"game":"uncharted-3","action":"noop"}}),
    ] {
        assert!(decode("pull", Game::U2, 3, Some(0), value.to_string().as_bytes()).is_err());
    }
    let err = json!({"ok":false,"command":"pull","error":{"code":"auth_failed","message":"sensitive-provider-detail"}});
    let message = decode("pull", Game::U2, 3, Some(4), err.to_string().as_bytes())
        .unwrap_err()
        .to_string();
    assert!(!message.contains("sensitive-provider-detail"));
    assert!(message.contains("sign in"));
    let held = decode(
        "pull",
        Game::U2,
        3,
        Some(6),
        report("held").to_string().as_bytes(),
    )
    .unwrap_err();
    assert!(held.to_string().contains("Another device"));
}

#[test]
fn responses_cannot_alias_another_slot_or_the_legacy_namespace() {
    for id in ["uncharted-2", "uncharted-2-slot-1", "uncharted-3-slot-3"] {
        let mut value = report("noop");
        value["data"]["game"] = id.into();
        assert!(decode("pull", Game::U2, 3, Some(0), value.to_string().as_bytes()).is_err());
    }
    assert!(decode(
        "pull",
        Game::U2,
        3,
        Some(0),
        report("noop").to_string().as_bytes()
    )
    .is_ok());
}

#[test]
fn conflict_event_round_trips() {
    let mut conflict = report("conflict");
    conflict["data"]["server"] =
        json!({"device":"tablet","time":"2026-11-17T00:00:00Z","revision":7});
    conflict["data"]["local"] =
        json!({"device":"desktop","time":"2026-11-16T00:00:00Z","revision":null});
    conflict["data"]["server_card"] = json!({"place":"Chapter two","playtime_secs":40});
    let Sync::Conflict(cards) = decode(
        "pull",
        Game::U2,
        3,
        Some(3),
        conflict.to_string().as_bytes(),
    )
    .unwrap() else {
        panic!("expected conflict");
    };
    let line = conflict_event("pull", &cards).to_string();
    let (game, parsed) = parse_conflict_event(&line).expect("parse conflict event");
    assert_eq!(game, Game::U2);
    assert_eq!(parsed.server.device, cards.server.device);
    assert_eq!(parsed.server.slot, cards.server.slot);
    assert_eq!(parsed.server.detail, cards.server.detail);
    assert_eq!(parsed.local.device, cards.local.device);
    assert_eq!(parse_conflict_event("not json"), None);
}
