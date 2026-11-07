use crate::hub::{HubConnection, HubError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_base64: Option<String>,
    #[serde(default)]
    pub avatar_color: String,
    #[serde(default)]
    pub about_me: Option<String>,
    #[serde(default)]
    pub is_online: bool,
    #[serde(default)]
    pub is_muted: bool,
    #[serde(default)]
    pub is_deafened: bool,
    #[serde(default)]
    pub is_server_muted: bool,
    #[serde(default)]
    pub is_server_deafened: bool,
    #[serde(default)]
    pub is_speaking: bool,
    #[serde(default)]
    pub current_channel_id: Option<String>,
    #[serde(default)]
    pub current_call_user_id: Option<String>,
    #[serde(default)]
    pub last_online: Option<String>,
    #[serde(default)]
    pub show_last_online: Option<bool>,
    #[serde(default)]
    pub is_streaming: bool,
    #[serde(default)]
    pub stream_quality: Option<String>,
    #[serde(default)]
    pub allow_remote_control: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    #[serde(default)]
    pub max_users: Option<u32>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInvite {
    pub sender_id: String,
    pub sender_name: String,
    pub channel_id: String,
    pub channel_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Captcha {
    #[serde(alias = "Id")]
    pub id: String,
    #[serde(rename = "imageBase64", alias = "ImageBase64")]
    pub image: String,
}

#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub password: Zeroizing<String>,
}

pub struct Registration {
    pub name: String,
    pub avatar: Option<String>,
    pub color: String,
    pub captcha_id: String,
    pub answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthError {
    Validation(&'static str),
    InvalidPassword,
    Throttled(u64),
    CaptchaRequired,
    CaptchaInvalid,
    CaptchaExpired,
    ClientRejected,
    Network,
}

impl AuthError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Validation(key) => key,
            Self::InvalidPassword => "validation.invalidPassword",
            Self::Throttled(_) => "validation.authThrottled",
            Self::CaptchaRequired => "validation.captchaRequired",
            Self::CaptchaInvalid => "validation.invalidCaptcha",
            Self::CaptchaExpired => "validation.expiredCaptcha",
            Self::ClientRejected => "main.connection.clientRejectedTitle",
            Self::Network => "validation.connectionError",
        }
    }
}

impl From<HubError> for AuthError {
    fn from(error: HubError) -> Self {
        if let HubError::Server(message) = error {
            if let Some((_, tail)) = message.split_once("ZABOR_AUTH_THROTTLED:") {
                let seconds = tail
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0);
                return Self::Throttled(seconds.div_ceil(60).max(1));
            }
            if message.contains("ZABOR_CLIENT_REJECTED") {
                return Self::ClientRejected;
            }
            if message.contains("CAPTCHA_REQUIRED") {
                return Self::CaptchaRequired;
            }
            if message.contains("CAPTCHA_EXPIRED") {
                return Self::CaptchaExpired;
            }
            if message.contains("CAPTCHA_INVALID") {
                return Self::CaptchaInvalid;
            }
        }
        Self::Network
    }
}

#[derive(Clone, Default)]
pub struct Bootstrap {
    pub user: User,
    pub channels: Vec<Channel>,
    pub friends: Vec<User>,
    pub requests: Vec<User>,
    pub invites: Vec<ChannelInvite>,
    pub audio_settings: Option<Value>,
    pub joke: String,
    pub ice_config: Option<Value>,
    pub channel_users: std::collections::BTreeMap<String, Vec<User>>,
    pub call_user: Option<User>,
}

pub enum LoginOutcome {
    NeedsRegistration,
    SignedIn(Box<Bootstrap>),
}

#[derive(Clone)]
pub struct AuthClient {
    hub: HubConnection,
}

impl AuthClient {
    pub fn new(hub: HubConnection) -> Self {
        Self { hub }
    }

    pub async fn login(&self, credentials: &Credentials) -> Result<LoginOutcome, AuthError> {
        self.login_with_resume(credentials, None).await
    }

    pub async fn login_with_resume(
        &self,
        credentials: &Credentials,
        session: Option<&str>,
    ) -> Result<LoginOutcome, AuthError> {
        self.login_with_resume_if(credentials, session, || true)
            .await
    }

    pub async fn login_with_resume_if(
        &self,
        credentials: &Credentials,
        session: Option<&str>,
        resume_is_current: impl Fn() -> bool,
    ) -> Result<LoginOutcome, AuthError> {
        validate_credentials(credentials)?;
        if session.is_some_and(|session| !crate::call_recovery::valid_session(session)) {
            return Err(AuthError::Network);
        }
        let exists = self
            .hub
            .invoke(
                "CheckUserExists",
                vec![json!(credentials.username)],
                Duration::from_secs(15),
            )
            .await?;
        match exists {
            Value::Bool(false) => return Ok(LoginOutcome::NeedsRegistration),
            Value::Bool(true) => {}
            _ => return Err(AuthError::Network),
        }
        let session = session.filter(|_| resume_is_current());
        let mut arguments = vec![
            json!(credentials.username),
            json!(credentials.password.as_str()),
        ];
        if let Some(session) = session {
            arguments.push(json!(session));
        }
        let value = self
            .hub
            .invoke(
                if session.is_some() {
                    "LoginAndResume"
                } else {
                    "Login"
                },
                arguments,
                Duration::from_secs(45),
            )
            .await?;
        if value.is_null() {
            return Err(AuthError::InvalidPassword);
        }
        let user: User = serde_json::from_value(value).map_err(|_| AuthError::Network)?;
        let _ = self
            .hub
            .invoke("EnableCallResume", vec![], Duration::from_secs(3))
            .await;
        Ok(LoginOutcome::SignedIn(Box::new(self.load(user).await?)))
    }

    pub async fn captcha(&self) -> Result<Captcha, AuthError> {
        let value = self
            .hub
            .invoke("GetRegistrationCaptcha", vec![], Duration::from_secs(15))
            .await?;
        let challenge: Captcha = serde_json::from_value(value).map_err(|_| AuthError::Network)?;
        if challenge.id.is_empty() || challenge.image.is_empty() {
            return Err(AuthError::Network);
        }
        Ok(challenge)
    }

    pub async fn register(
        &self,
        credentials: &Credentials,
        registration: Registration,
    ) -> Result<Bootstrap, AuthError> {
        validate_credentials(credentials)?;
        validate_name(&registration.name)?;
        if registration.captcha_id.is_empty() || registration.answer.trim().is_empty() {
            return Err(AuthError::CaptchaRequired);
        }
        let value = self
            .hub
            .invoke(
                "Register",
                vec![
                    json!(credentials.username),
                    json!(credentials.password.as_str()),
                    json!(registration.name.trim()),
                    json!(registration.avatar),
                    json!(registration.color),
                    json!(registration.captcha_id),
                    json!(registration.answer.trim()),
                ],
                Duration::from_secs(15),
            )
            .await?;
        let user: User = serde_json::from_value(value).map_err(|_| AuthError::Network)?;
        self.load(user).await
    }

    pub async fn load(&self, user: User) -> Result<Bootstrap, AuthError> {
        if user.id.is_empty() {
            return Err(AuthError::Network);
        }
        let duration = Duration::from_secs(20);
        let (channels, friends, requests, invites) = tokio::try_join!(
            self.hub.invoke("GetChannels", vec![], duration),
            self.hub.invoke("GetFriends", vec![], duration),
            self.hub.invoke("GetFriendRequests", vec![], duration),
            self.hub.invoke("GetChannelInvites", vec![], duration),
        )?;
        let (settings, joke, ice) = tokio::join!(
            self.hub.invoke("GetAudioSettings", vec![], duration),
            self.hub.invoke("GetJokeOfTheDay", vec![], duration),
            self.hub
                .invoke("GetIceServers", vec![], Duration::from_secs(5))
        );
        Ok(Bootstrap {
            user,
            channels: serde_json::from_value(channels).map_err(|_| AuthError::Network)?,
            friends: serde_json::from_value(friends).map_err(|_| AuthError::Network)?,
            requests: serde_json::from_value(requests).map_err(|_| AuthError::Network)?,
            invites: serde_json::from_value(invites).map_err(|_| AuthError::Network)?,
            audio_settings: settings.ok().and_then(|value| {
                value
                    .as_str()
                    .and_then(|text| serde_json::from_str(text).ok())
            }),
            joke: joke
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            ice_config: ice.ok(),
            channel_users: Default::default(),
            call_user: None,
        })
    }
}

pub fn validate_credentials(credentials: &Credentials) -> Result<(), AuthError> {
    for value in [credentials.username.as_str(), credentials.password.as_str()] {
        if value.encode_utf16().count() < 4 {
            return Err(AuthError::Validation("validation.minChars"));
        }
        if value.encode_utf16().count() > 25 {
            return Err(AuthError::Validation("validation.maxChars"));
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"!@#$%^&*()_+={}[]:;\"'<>,.?/\\|-".contains(&byte)
        }) {
            return Err(AuthError::Validation("validation.latinOnly"));
        }
    }
    Ok(())
}

pub fn validate_name(value: &str) -> Result<(), AuthError> {
    if value.trim().is_empty() {
        return Err(AuthError::Validation("validation.emptyName"));
    }
    if value.encode_utf16().count() > 20 {
        return Err(AuthError::Validation("validation.maxNameChars"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_and_display_name_validation_match_legacy_limits() {
        let mut credentials = Credentials {
            username: "name".into(),
            password: Zeroizing::new("p@ssword".into()),
        };
        assert_eq!(validate_credentials(&credentials), Ok(()));
        credentials.username = "имяuser".into();
        assert_eq!(
            validate_credentials(&credentials),
            Err(AuthError::Validation("validation.latinOnly"))
        );
        assert_eq!(
            validate_name(&"😀".repeat(11)),
            Err(AuthError::Validation("validation.maxNameChars"))
        );
        assert_eq!(validate_name("Виктор"), Ok(()));
    }

    #[test]
    fn server_errors_are_classified_without_displaying_raw_payloads() {
        assert_eq!(
            AuthError::from(HubError::Server("CAPTCHA_EXPIRED".into())),
            AuthError::CaptchaExpired
        );
        assert_eq!(
            AuthError::from(HubError::Server("ZABOR_AUTH_THROTTLED:30".into())),
            AuthError::Throttled(1)
        );
        assert_eq!(AuthError::from(HubError::Network), AuthError::Network);
    }
}
