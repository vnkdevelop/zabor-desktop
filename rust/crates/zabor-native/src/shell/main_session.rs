use super::{AuthClient, AuthError, Command, Credentials, LoginOutcome, UiEvent};
use std::time::Duration;
use zabor_native::auth::User;
use zabor_native::hub::HubConnection;
use zabor_native::roster::{respond_notification, NotificationKind};

pub(super) enum MainResult {
    Authentication(LoginOutcome, Credentials),
    Event(UiEvent),
    Profile(super::UserProfileResult),
}

pub(super) fn is_session_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Authenticate { .. }
            | Command::Captcha
            | Command::Modal { .. }
            | Command::UpdateProfile(..)
            | Command::LookupProfile(..)
            | Command::ProfileAddFriend(..)
            | Command::Notification(
                NotificationKind::Friend | NotificationKind::Channel,
                _,
                false
            )
            | Command::Notification(NotificationKind::Friend, _, true)
    )
}

pub(super) async fn compute(
    command: Command,
    auth: &AuthClient,
    connection: &HubConnection,
    user: Option<User>,
) -> Result<MainResult, AuthError> {
    match command {
        Command::Authenticate {
            credentials,
            registration,
        } => {
            let outcome = match registration {
                Some(registration) => auth
                    .register(&credentials, registration)
                    .await
                    .map(|data| LoginOutcome::SignedIn(Box::new(data))),
                None => auth.login(&credentials).await,
            }?;
            Ok(MainResult::Authentication(outcome, credentials))
        }
        Command::Captcha => auth
            .captcha()
            .await
            .map(|captcha| MainResult::Event(UiEvent::Captcha(captcha))),
        Command::LookupProfile(query) => {
            let result = if user.as_ref().is_some_and(|user| user.id == query.owner)
                && query.id != query.owner
            {
                tokio::select! {
                    biased;
                    _ = query.stop.cancelled() => Err(AuthError::Network),
                    result = auth.get_user_by_username(&query.username) => result,
                }
            } else {
                Err(AuthError::Network)
            };
            Ok(MainResult::Event(UiEvent::ProfileLookup(query, result)))
        }
        Command::ProfileAddFriend(query) => {
            let sent = if user.as_ref().is_some_and(|user| user.id == query.owner)
                && query.id != query.owner
            {
                tokio::select! {
                    biased;
                    _ = query.stop.cancelled() => false,
                    result = connection.invoke("SendFriendRequest", vec![serde_json::json!(query.username)], Duration::from_secs(20)) => matches!(result, Ok(serde_json::Value::Bool(true))),
                }
            } else {
                false
            };
            Ok(MainResult::Event(UiEvent::ProfileFriendResult(query, sent)))
        }
        Command::UpdateProfile(update, query) => {
            let user = user.ok_or(AuthError::Network)?;
            let result = tokio::select! {
                biased;
                _ = query.stop.cancelled() => return Err(AuthError::Network),
                result = auth.update_profile(&update, &user) => result,
            }?;
            if !query.alive() {
                return Err(AuthError::Network);
            }
            Ok(MainResult::Profile(super::UserProfileResult {
                user: result,
                query,
            }))
        }
        Command::Notification(kind, id, accept) => {
            if !respond_notification(connection, kind, &id, accept)
                .await
                .map_err(AuthError::from)?
            {
                return Err(AuthError::Network);
            }
            Ok(MainResult::Event(UiEvent::NotificationResolved(
                kind, id, accept,
            )))
        }
        Command::Modal { kind, value } => {
            let target = if kind == 1 {
                "CreateChannel"
            } else {
                "SendFriendRequest"
            };
            if value.trim().is_empty() || value.encode_utf16().count() > 25 {
                return Err(AuthError::Validation("validation.maxChars"));
            }
            connection
                .invoke(
                    target,
                    vec![serde_json::json!(value.trim())],
                    Duration::from_secs(20),
                )
                .await
                .map_err(AuthError::from)?;
            let user = user.ok_or(AuthError::Network)?;
            auth.load(user)
                .await
                .map(|data| MainResult::Event(UiEvent::Data(Box::new(data))))
        }
        _ => unreachable!(),
    }
}
