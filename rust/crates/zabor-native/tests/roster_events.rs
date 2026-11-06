use serde_json::{json, Value};
use zabor_native::auth::{Bootstrap, User};
use zabor_native::hub::HubEvent;
use zabor_native::roster::apply_event;

fn user(id: &str) -> User {
    User {
        id: id.into(),
        username: id.into(),
        display_name: id.into(),
        is_online: true,
        ..User::default()
    }
}

fn event(target: &str, arguments: Vec<Value>) -> HubEvent {
    HubEvent {
        target: target.into(),
        arguments,
    }
}

fn state() -> Bootstrap {
    Bootstrap {
        user: user("owner"),
        friends: vec![user("alice"), user("bob")],
        ..Bootstrap::default()
    }
}

#[test]
fn channel_switch_rejects_late_departure_and_repeated_membership() {
    let mut state = state();
    for channel in ["old", "new", "new"] {
        assert!(apply_event(
            &mut state,
            &event(
                "UserJoinedChannel",
                vec![json!(user("alice")), json!(channel)]
            )
        ));
    }
    assert!(apply_event(
        &mut state,
        &event("UserSpeaking", vec![json!("alice"), json!(true)])
    ));
    assert!(apply_event(
        &mut state,
        &event("UserLeftChannel", vec![json!("alice"), json!("old")])
    ));
    assert!(state.channel_users["old"].is_empty());
    assert_eq!(state.channel_users["new"].len(), 1);
    assert_eq!(state.friends[0].current_channel_id.as_deref(), Some("new"));
    assert!(state.friends[0].is_speaking);
    assert!(apply_event(
        &mut state,
        &event(
            "UserLeft",
            vec![json!("alice"), json!("2026-11-06T12:00:00Z")]
        )
    ));
    assert!(state.channel_users["new"].is_empty());
    assert!(!state.friends[0].is_online);
    assert!(!state.friends[0].is_speaking);
    assert_eq!(state.friends[0].current_channel_id, None);
    assert_eq!(
        state.friends[0].last_online.as_deref(),
        Some("2026-11-06T12:00:00Z")
    );
}

#[test]
fn full_snapshot_replaces_occupancy_atomically_and_rejects_invalid_members() {
    let mut state = state();
    apply_event(
        &mut state,
        &event(
            "UserJoinedChannel",
            vec![json!(user("alice")), json!("old")],
        ),
    );
    let snapshot = json!({"new":[user("bob"), user("owner")]});
    assert!(apply_event(
        &mut state,
        &event("SyncFullChannelState", vec![snapshot])
    ));
    assert_eq!(state.channel_users.len(), 1);
    assert_eq!(state.channel_users["new"].len(), 2);
    assert_eq!(state.user.current_channel_id.as_deref(), Some("new"));
    assert_eq!(state.friends[0].current_channel_id, None);
    let previous = state.channel_users.clone();
    for invalid in [
        json!({"new":[user("bob"), user("bob")]}),
        json!({"new":[user("bob")],"other":[user("bob")]}),
        json!({"new":[{"id":"broken"}]}),
        Value::Null,
    ] {
        assert!(!apply_event(
            &mut state,
            &event("SyncFullChannelState", vec![invalid])
        ));
        assert_eq!(state.channel_users, previous);
    }
    assert!(apply_event(
        &mut state,
        &event("SyncFullChannelState", vec![json!({})])
    ));
    assert!(state.channel_users.is_empty());
    assert_eq!(state.user.current_channel_id, None);
}

#[test]
fn friend_requests_additions_and_removals_are_idempotent() {
    let mut state = state();
    for _ in 0..2 {
        assert!(apply_event(
            &mut state,
            &event("FriendRequestReceived", vec![json!(user("carol"))])
        ));
    }
    assert_eq!(state.requests.len(), 1);
    for target in ["FriendRequestAccepted", "FriendAdded", "FriendAdded"] {
        assert!(apply_event(
            &mut state,
            &event(target, vec![json!(user("carol"))])
        ));
    }
    assert!(state.requests.is_empty());
    assert_eq!(state.friends.len(), 3);
    apply_event(
        &mut state,
        &event(
            "UserJoinedChannel",
            vec![json!(user("carol")), json!("channel")],
        ),
    );
    apply_event(&mut state, &event("FriendRemoved", vec![json!("carol")]));
    assert_eq!(state.friends.len(), 2);
    assert_eq!(state.channel_users["channel"].len(), 1);
    apply_event(&mut state, &event("UserDeleted", vec![json!("carol")]));
    assert!(state.channel_users["channel"].is_empty());
    assert!(!apply_event(
        &mut state,
        &event("FriendAdded", vec![json!({"id":"untrusted"})])
    ));
    assert!(!apply_event(
        &mut state,
        &event("FriendAdded", vec![json!(user("owner"))])
    ));
    assert_eq!(state.friends.len(), 2);
}

#[test]
fn moderation_and_speaking_preserve_local_mute_and_partial_remote_state() {
    let mut state = state();
    state.user.is_muted = true;
    state.friends[0].is_deafened = true;
    let snapshot = json!({"channel":[state.user.clone(), state.friends[0].clone()]});
    apply_event(&mut state, &event("SyncFullChannelState", vec![snapshot]));
    assert!(apply_event(
        &mut state,
        &event(
            "UserStateChanged",
            vec![
                json!({"UserId":"owner","IsMuted":false,"IsServerDeafened":true,"IsSpeaking":false})
            ]
        )
    ));
    assert!(state.user.is_muted);
    assert!(state.user.is_server_deafened);
    assert!(state.channel_users["channel"][0].is_muted);
    assert!(apply_event(
        &mut state,
        &event(
            "UserStateChanged",
            vec![json!({"userId":"alice","isSpeaking":true})]
        )
    ));
    assert!(state.friends[0].is_deafened);
    assert!(state.friends[0].is_speaking);
    assert!(state.channel_users["channel"][1].is_speaking);
    assert!(apply_event(
        &mut state,
        &event(
            "UserStateChanged",
            vec![json!({"userId":"owner","isServerDeafened":false})]
        )
    ));
    assert!(!state.user.is_server_deafened);
    assert!(state.user.is_muted);
}

#[test]
fn channel_delete_cleans_membership_and_related_invites() {
    let mut state = state();
    let channel = json!({"id":"channel","name":"first","ownerId":"owner"});
    for _ in 0..2 {
        apply_event(&mut state, &event("ChannelCreated", vec![channel.clone()]));
    }
    assert_eq!(state.channels.len(), 1);
    apply_event(
        &mut state,
        &event(
            "ChannelUpdated",
            vec![json!({"id":"channel","name":"renamed","ownerId":"owner"})],
        ),
    );
    assert_eq!(state.channels[0].name, "renamed");
    apply_event(
        &mut state,
        &event(
            "UserJoinedChannel",
            vec![json!(user("alice")), json!("channel")],
        ),
    );
    let invite = json!({"senderId":"alice","senderName":"Alice","channelId":"channel","channelName":"renamed"});
    for _ in 0..2 {
        apply_event(
            &mut state,
            &event("ReceiveChannelInvite", vec![invite.clone()]),
        );
    }
    assert_eq!(state.invites.len(), 1);
    apply_event(&mut state, &event("ChannelDeleted", vec![json!("channel")]));
    assert!(state.channels.is_empty());
    assert!(state.invites.is_empty());
    assert!(state.channel_users.is_empty());
    assert_eq!(state.friends[0].current_channel_id, None);
}

#[test]
fn direct_call_profile_and_stream_events_update_without_channel_membership() {
    let mut state = state();
    state.call_user = Some(user("caller"));
    apply_event(
        &mut state,
        &event(
            "UserStartedStreaming",
            vec![json!("caller"), json!("medium")],
        ),
    );
    assert!(state.call_user.as_ref().unwrap().is_streaming);
    assert!(state.call_user.as_ref().unwrap().allow_remote_control);
    apply_event(
        &mut state,
        &event("UserStoppedStreaming", vec![json!("caller")]),
    );
    assert!(!state.call_user.as_ref().unwrap().is_streaming);
    assert!(!state.call_user.as_ref().unwrap().allow_remote_control);
    let mut updated = user("caller");
    updated.display_name = "Renamed".into();
    apply_event(&mut state, &event("UserUpdated", vec![json!(updated)]));
    assert_eq!(state.call_user.as_ref().unwrap().display_name, "Renamed");
}
