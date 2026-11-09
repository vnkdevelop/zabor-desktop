use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use zabor_native::auth::{AuthError, Channel, User};
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubConnection;

#[derive(Clone)]
pub(super) enum Operation {
    Members(Channel),
    Invite { channel: Channel, target: Box<User> },
    Kick { channel: Channel, target: String },
    RemoveFriend(String),
}

#[derive(Clone)]
pub(super) struct Request {
    identity: Arc<()>,
    pub(super) owner: String,
    pub(super) ticket: IntentTicket,
    pub(super) stop: CancellationToken,
    pub(super) operation: Operation,
}

pub(super) enum Response {
    Members(Vec<User>),
    Confirmed,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control)
}

impl Request {
    pub(super) fn new(
        owner: &str,
        operation: Operation,
        intents: &CriticalIntents,
    ) -> Option<Self> {
        if !valid_id(owner) {
            return None;
        }
        let (channel, target) = match &operation {
            Operation::Members(channel) => (Some(channel), None),
            Operation::Invite { channel, target } => (Some(channel), Some(target.id.as_str())),
            Operation::Kick { channel, target } => (Some(channel), Some(target.as_str())),
            Operation::RemoveFriend(target) => (None, Some(target.as_str())),
        };
        if channel.is_some_and(|channel| {
            !valid_id(&channel.id)
                || channel.name.trim().is_empty()
                || channel.name.encode_utf16().count() > 25
        }) || target.is_some_and(|target| !valid_id(target) || target == owner)
        {
            return None;
        }
        let ticket = intents.ticket();
        let stop = if matches!(operation, Operation::Invite { .. }) {
            intents.voice_token(ticket)
        } else {
            intents.session_token(ticket)
        };
        (!stop.is_cancelled()).then(|| Self {
            identity: Arc::new(()),
            owner: owner.into(),
            ticket,
            stop,
            operation,
        })
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    pub(super) fn scope(&self) -> IntentScope {
        if matches!(self.operation, Operation::Invite { .. }) {
            IntentScope::Voice
        } else {
            IntentScope::Session
        }
    }
}

pub(super) async fn execute(
    request: &Request,
    connection: &HubConnection,
    user: &User,
    friend: bool,
    current_channel: Option<&str>,
    intents: &CriticalIntents,
) -> Result<Response, AuthError> {
    if request.owner != user.id || request.stop.is_cancelled() {
        return Err(AuthError::Network);
    }
    let (method, arguments, timeout) = match &request.operation {
        Operation::Members(channel) => (
            "GetChannelMembersList",
            vec![serde_json::json!(channel.id)],
            Duration::from_secs(6),
        ),
        Operation::Invite { channel, target }
            if target.is_online && current_channel == Some(channel.id.as_str()) =>
        {
            (
                "CallToChannel",
                vec![
                    serde_json::json!(target.id),
                    serde_json::json!(channel.id),
                    serde_json::json!(channel.name),
                ],
                Duration::from_secs(20),
            )
        }
        Operation::Kick { channel, target } if channel.owner_id == user.id => (
            "KickFromChannel",
            vec![serde_json::json!(channel.id), serde_json::json!(target)],
            Duration::from_secs(20),
        ),
        Operation::RemoveFriend(target) if friend => (
            "RemoveFriend",
            vec![serde_json::json!(target)],
            Duration::from_secs(20),
        ),
        _ => return Err(AuthError::Network),
    };
    let result = tokio::select! {
        biased;
        _ = request.stop.cancelled() => return Err(AuthError::Network),
        result = intents.run(request.ticket, request.scope(), connection.invoke(method, arguments, timeout)) => result,
    }
    .ok_or(AuthError::Network)?
    .map_err(AuthError::from)?;
    if request.stop.is_cancelled() || !intents.is_current(request.ticket, request.scope()) {
        return Err(AuthError::Network);
    }
    if matches!(request.operation, Operation::Members(_)) {
        parse_members(result).map(Response::Members)
    } else if result == serde_json::Value::Bool(true) {
        Ok(Response::Confirmed)
    } else {
        Err(AuthError::Network)
    }
}

fn parse_members(result: serde_json::Value) -> Result<Vec<User>, AuthError> {
    if serde_json::to_vec(&result)
        .map_err(|_| AuthError::Network)?
        .len()
        > 2 * 1024 * 1024
    {
        return Err(AuthError::Network);
    }
    let users: Vec<User> = serde_json::from_value(result).map_err(|_| AuthError::Network)?;
    let mut ids = BTreeSet::new();
    if users.len() > 4096
        || users.iter().any(|user| {
            !valid_id(&user.id)
                || user.username.trim().is_empty()
                || user.username.encode_utf16().count() > 25
                || !ids.insert(user.id.as_str())
        })
    {
        return Err(AuthError::Network);
    }
    Ok(users)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zabor_native::critical_intents::CriticalAction;

    fn channel() -> Channel {
        Channel {
            id: "channel".into(),
            name: "Channel".into(),
            owner_id: "owner".into(),
            ..Default::default()
        }
    }

    #[test]
    fn original_member_and_friend_tickets_survive_leave_but_invites_and_all_old_owners_are_revoked()
    {
        let intents = CriticalIntents::default();
        let members = Request::new("owner", Operation::Members(channel()), &intents).unwrap();
        let copy = members.clone();
        let friend =
            Request::new("owner", Operation::RemoveFriend("peer".into()), &intents).unwrap();
        let invite = Request::new(
            "owner",
            Operation::Invite {
                channel: channel(),
                target: Box::new(User {
                    id: "peer".into(),
                    ..Default::default()
                }),
            },
            &intents,
        )
        .unwrap();
        intents.request(CriticalAction::Leave);
        assert!(members.same(&copy));
        assert_eq!(members.ticket, copy.ticket);
        assert!(!members.stop.is_cancelled() && !friend.stop.is_cancelled());
        assert!(invite.stop.is_cancelled());
        intents.request(CriticalAction::Logout);
        assert!(members.stop.is_cancelled() && friend.stop.is_cancelled());
        assert!(Request::new("owner", Operation::Members(channel()), &intents).is_none());
    }

    #[test]
    fn member_shapes_sizes_duplicates_and_invalid_operation_targets_fail_closed() {
        let user = User {
            id: "member".into(),
            username: "Member".into(),
            display_name: "Member".into(),
            ..Default::default()
        };
        assert_eq!(
            parse_members(serde_json::json!([&user]))
                .unwrap()
                .as_slice(),
            std::slice::from_ref(&user)
        );
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!([user.clone(), user.clone()]),
            serde_json::json!([{"id":"","username":"Member","displayName":"Member"}]),
            serde_json::json!([{"id":"member","username":" ","displayName":"Member"}]),
        ] {
            assert!(parse_members(value).is_err());
        }
        let mut oversized = user;
        oversized.avatar_base64 = Some("x".repeat(2 * 1024 * 1024));
        assert!(parse_members(serde_json::json!([oversized])).is_err());
        let intents = CriticalIntents::default();
        for target in ["", "owner", "bad\n"] {
            assert!(
                Request::new("owner", Operation::RemoveFriend(target.into()), &intents).is_none()
            );
        }
    }
}
