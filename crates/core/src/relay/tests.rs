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

#[test]
fn conflict_event_phase_and_sanitization() {
    let raw = json!({
        "version": 1,
        "event": "save-conflict",
        "phase": "push",
        "game": "u3",
        "server": {
            "device": "  \x1b[31mDeck\n  ",
            "saved_unix": 200,
            "detail": "\nChapter 7\t",
            "play_time_secs": 90,
            "slot": "Revision\x07 9"
        },
        "local": {
            "device": "   ",
            "saved_unix": 100,
            "detail": "\x1b\n",
            "slot": ""
        }
    })
    .to_string();
    let (game, phase, pair) = parse_conflict_event_with_phase(&raw).expect("valid push conflict");
    assert_eq!(game, Game::U3);
    assert_eq!(phase, "push");
    assert!(pair.server_is_newer);
    assert_eq!(pair.server.device.as_deref(), Some("[31mDeck"));
    assert_eq!(pair.server.detail.as_deref(), Some("Chapter 7"));
    assert_eq!(pair.server.slot, "Revision 9");
    assert_eq!(pair.local.device, None);
    assert_eq!(pair.local.detail, None);

    // Reject unknown version, unknown phase, or non-object card.
    assert_eq!(
        parse_conflict_event_with_phase(
            &json!({"version": 2, "event": "save-conflict", "phase": "pull", "game": "u1", "server": {"saved_unix": 1}, "local": {"saved_unix": 1}}).to_string()
        ),
        None
    );
    assert_eq!(
        parse_conflict_event_with_phase(
            &json!({"version": 1, "event": "save-conflict", "phase": "sync", "game": "u1", "server": {"saved_unix": 1}, "local": {"saved_unix": 1}}).to_string()
        ),
        None
    );
    assert_eq!(
        parse_conflict_event_with_phase(
            &json!({"version": 1, "event": "save-conflict", "phase": "pull", "game": "u1", "server": null, "local": {"saved_unix": 1}}).to_string()
        ),
        None
    );
}

#[test]
fn decode_handles_not_set_up_and_credential_store_errors() {
    let not_set_up = json!({"ok": false, "command": "pull", "error": {"code": "not_set_up"}});
    assert_eq!(
        decode(
            "pull",
            Game::U1,
            1,
            Some(4),
            not_set_up.to_string().as_bytes()
        )
        .unwrap(),
        Sync::LocalOnly("Save Relay needs setup; using local saves.")
    );
    for code in [
        "credential_store_unavailable",
        "credential_store_locked",
        "keyring_locked",
        "secret_service_unavailable",
    ] {
        let err = json!({
            "ok": false,
            "command": "pull",
            "error": {"code": code, "message": "secret-dbus-path"}
        });
        let msg = decode("pull", Game::U1, 1, Some(4), err.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(msg.contains("credential store"), "{code}: {msg}");
        assert!(!msg.contains("secret-dbus-path"));
    }
}

#[test]
fn cloud_service_ids_and_names_round_trip() {
    for (svc, id, name) in [
        (CloudService::Nextcloud, "nextcloud", "Nextcloud"),
        (CloudService::GoogleDrive, "gdrive", "Google Drive"),
        (CloudService::Dropbox, "dropbox", "Dropbox"),
    ] {
        assert_eq!(svc.id(), id);
        assert_eq!(svc.name(), name);
        assert_eq!(CloudService::from_id(id), Some(svc));
    }
    assert_eq!(
        CloudService::from_id("google-drive"),
        Some(CloudService::GoogleDrive)
    );
    assert_eq!(CloudService::from_id("unknown"), None);
}

#[test]
fn decode_status_handles_configured_unconfigured_and_errors() {
    let ok = json!({"ok": true, "command": "status", "data": {"service": "nextcloud", "account": "nathan", "configured": true}});
    assert_eq!(
        decode_status(Some(0), ok.to_string().as_bytes()).unwrap(),
        RelayAccountStatus {
            service: Some(CloudService::Nextcloud),
            account: Some("nathan".into()),
            configured: true,
        }
    );
    let unconfigured = json!({"ok": false, "command": "status", "error": {"code": "not_set_up"}});
    assert_eq!(
        decode_status(Some(4), unconfigured.to_string().as_bytes()).unwrap(),
        RelayAccountStatus::default()
    );
    let locked = json!({"ok": false, "command": "status", "error": {"code": "keyring_locked", "message": "private-keyring-id"}});
    let msg = decode_status(Some(4), locked.to_string().as_bytes())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("credential store"));
    assert!(!msg.contains("private-keyring-id"));
    assert!(decode_status(Some(0xc0000135_u32 as i32), b"")
        .unwrap_err()
        .to_string()
        .contains("Visual C++ x64"));
    let wrong_cmd = json!({"ok": true, "command": "pull", "data": {}});
    assert!(decode_status(Some(0), wrong_cmd.to_string().as_bytes()).is_err());
}

#[test]
fn decode_setup_and_sign_out_responses() {
    let nc_ok = json!({"ok": true, "command": "setup", "data": {"account": "elena"}});
    assert_eq!(
        decode_sign_in_nextcloud("nathan", true, Some(0), nc_ok.to_string().as_bytes()).unwrap(),
        RelayAccountStatus {
            service: Some(CloudService::Nextcloud),
            account: Some("elena".into()),
            configured: true,
        }
    );
    let nc_locked = json!({"ok": false, "error": {"code": "credential_store_unavailable", "message": "secret-token"}});
    let err = decode_sign_in_nextcloud("nathan", false, Some(4), nc_locked.to_string().as_bytes())
        .unwrap_err()
        .to_string();
    assert!(err.contains("credential store"));
    assert!(!err.contains("secret-token"));
    let nc_offline = json!({"ok": false, "error": {"code": "server_unreachable"}});
    assert!(
        decode_sign_in_nextcloud("nathan", false, Some(5), nc_offline.to_string().as_bytes())
            .unwrap_err()
            .to_string()
            .contains("Could not reach the save sync server")
    );

    let oauth_ok = json!({
        "ok": true,
        "data": {
            "verification_url": "https://example.invalid/device",
            "user_code": "WXYZ-1234",
            "qr": "██ ██\n  █  \r\n██ ██"
        }
    });
    assert_eq!(
        decode_start_oauth(
            CloudService::GoogleDrive,
            true,
            Some(0),
            oauth_ok.to_string().as_bytes()
        )
        .unwrap(),
        RelayChallenge {
            service: CloudService::GoogleDrive,
            verification_url: "https://example.invalid/device".into(),
            user_code: Some("WXYZ-1234".into()),
            qr: Some("██ ██\n  █  \n██ ██".into()),
        }
    );
    let oauth_empty_url = json!({"ok": true, "data": {"verification_url": "   "}});
    assert!(decode_start_oauth(
        CloudService::Dropbox,
        true,
        Some(0),
        oauth_empty_url.to_string().as_bytes()
    )
    .is_err());
    assert!(decode_start_oauth(
        CloudService::Dropbox,
        false,
        Some(4),
        nc_locked.to_string().as_bytes()
    )
    .unwrap_err()
    .to_string()
    .contains("credential store"));

    assert!(decode_sign_out(true, Some(0), br#"{"ok":true}"#).is_ok());
    assert!(
        decode_sign_out(false, Some(4), nc_locked.to_string().as_bytes())
            .unwrap_err()
            .to_string()
            .contains("credential store")
    );
}

#[test]
fn desktop_relay_validates_setup_inputs_before_spawning() {
    let relay = DesktopRelay {
        program: PathBuf::from("/nonexistent/save-relay"),
        manifests: PathBuf::from("/nonexistent"),
    };
    assert!(relay
        .sign_in_nextcloud("", "user", "pw")
        .unwrap_err()
        .to_string()
        .contains("Nextcloud address"));
    assert!(relay
        .sign_in_nextcloud("https://nc.invalid", "user", "line1\nline2")
        .unwrap_err()
        .to_string()
        .contains("line breaks"));
    assert!(relay
        .start_oauth(CloudService::Nextcloud)
        .unwrap_err()
        .to_string()
        .contains("Nextcloud sign-in"));
}
