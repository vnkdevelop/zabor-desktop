use super::{AuthClient, AuthError, Command, Credentials, LoginOutcome, UiEvent};
use std::time::Duration;
use zabor_native::auth::User;
use zabor_native::hub::HubConnection;
use zabor_native::roster::{respond_notification, NotificationKind};

pub(super) enum MainResult {
    Authentication(LoginOutcome, Credentials),
    Event(UiEvent),
}

pub(super) fn is_session_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Authenticate { .. }
            | Command::Captcha
            | Command::Modal { .. }
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
