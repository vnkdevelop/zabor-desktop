use super::*;

#[test]
fn member_invite_callbacks_follow_actual_original_join_in_ru_en_and_reject_closed_view_result() {
    for language in ["ru", "en"] {
        for mode in ["true", "false", "null", "closed"] {
            std::thread::spawn(move || {
                let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
                ui.set_language(language.into());
                ui.set_user_id("fixture-owner".into());
                data.borrow_mut().user.id = "fixture-owner".into();
                data.borrow_mut().channels[0] = Channel {
                    id: "fixture-channel".into(),
                    name: "Fixture".into(),
                    owner_id: "fixture-owner".into(),
                    ..Default::default()
                };
                assert!(!ui.get_voice_active());
                assert!(ui.get_active_channel().is_empty());
                assert_ne!(ui.invoke_tr("profile.inviteToChannel".into(), language.into()), "profile.inviteToChannel");
                let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
                let event = runtime.block_on(super::super::super::main_session_tests::member_origin_invite_original_worker(
                    intents.clone(),
                    mode,
                    |channel, users| {
                        assert_eq!(channel, "fixture-channel");
                        assert_eq!(users.len(), 1);
                        assert_eq!(users[0].id, data.borrow().user.id);
                        ui.set_voice_active(true);
                        ui.set_active_channel(channel.clone().into());
                        ui.invoke_open_channel_members(channel.into());
                        let Command::MemberAction(query) = receiver.try_recv().unwrap().command else { panic!("post-Join original member query missing") };
                        let peer = User { id: "fixture-peer".into(), is_online: true, ..member() };
                        state.borrow_mut().handle(&ui, &app, &data.borrow(), Event { request: query, result: Ok(Response::Members(vec![peer])) }, &intents);
                        ui.invoke_open_member_profile("fixture-peer".into());
                        let Command::LookupProfile(_) = receiver.try_recv().unwrap().command else { panic!("post-Join original member lookup missing") };
                        ui.invoke_member_profile_action("invite".into());
                        let queued = receiver.try_recv().unwrap();
                        let Command::MemberAction(original) = queued.command else { panic!("post-Join original MemberUi Invite missing") };
                        assert_eq!(queued.ticket, original.ticket);
                        assert!(ui.get_member_action_pending());
                        assert!(ui.get_member_invited());
                        ui.invoke_member_profile_action("invite".into());
                        assert!(receiver.try_recv().is_err());
                        original
                    },
                    || {
                        if mode == "closed" { ui.invoke_close_member_profile(); }
                    },
                ));
                let closed = mode == "closed";
                assert_eq!(event.request.stop.is_cancelled(), closed);
                assert_eq!(matches!(event.result, Ok(Response::Confirmed)), mode == "true");
                state.borrow_mut().handle(&ui, &app, &data.borrow(), event, &intents);
                assert!(!ui.get_member_action_pending());
                assert_eq!(ui.get_member_invited(), mode == "true");
                assert_eq!(ui.get_modal(), if closed { 11 } else { 9 });
                if mode == "true" {
                    ui.invoke_member_profile_action("invite".into());
                    assert!(receiver.try_recv().is_err());
                }
                assert!(data.borrow().friends.is_empty());
                assert!(data.borrow().channel_users.is_empty());
                println!("member_invite_ui language={language} mode={mode} actual_voice_event=true original_callbacks=true original_request=true closed_response_rejected={closed} permission_roster_unchanged=true physical_audio=false");
            }).join().unwrap();
        }
    }
}
