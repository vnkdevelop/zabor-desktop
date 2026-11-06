use crate::auth::{Bootstrap, Channel, ChannelInvite, User};
use crate::hub::HubEvent;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    #[serde(alias = "UserId")]
    pub user_id: String,
    #[serde(alias = "IsMuted")]
    pub is_muted: Option<bool>,
    #[serde(alias = "IsDeafened")]
    pub is_deafened: Option<bool>,
    #[serde(alias = "IsServerMuted")]
    pub is_server_muted: Option<bool>,
    #[serde(alias = "IsServerDeafened")]
    pub is_server_deafened: Option<bool>,
    #[serde(alias = "IsSpeaking")]
    pub is_speaking: Option<bool>,
}

impl AudioState {
    pub fn apply(&self, user: &mut User, local: bool) {
        if !local {
            if let Some(value) = self.is_muted {
                user.is_muted = value;
            }
            if let Some(value) = self.is_deafened {
                user.is_deafened = value;
            }
        }
        if let Some(value) = self.is_server_muted {
            user.is_server_muted = value;
        }
        if let Some(value) = self.is_server_deafened {
            user.is_server_deafened = value;
        }
        if let Some(value) = self.is_speaking {
            user.is_speaking = value;
        }
    }
}

pub fn apply_user_event(user: &mut User, event: &HubEvent, local: bool) -> bool {
    let first = event.arguments.first().unwrap_or(&Value::Null);
    match event.target.as_str() {
        "UserUpdated" | "UserJoined" => {
            let Ok(mut updated) = serde_json::from_value::<User>(first.clone()) else {
                return false;
            };
            if updated.id != user.id {
                return false;
            }
            if event.target == "UserJoined" {
                updated.is_online = true;
            }
            *user = updated;
        }
        "UserStateChanged" => {
            let Ok(state) = serde_json::from_value::<AudioState>(first.clone()) else {
                return false;
            };
            if state.user_id != user.id {
                return false;
            }
            state.apply(user, local);
        }
        "UserLeft" if first.as_str() == Some(&user.id) => {
            user.is_online = false;
            user.current_channel_id = None;
            user.current_call_user_id = None;
            user.is_speaking = false;
            user.is_streaming = false;
            user.stream_quality = None;
            user.allow_remote_control = false;
            if let Some(last_online) = event.arguments.get(1).and_then(Value::as_str) {
                user.last_online = Some(last_online.into());
            }
        }
        "UserSpeaking" if first.as_str() == Some(&user.id) => {
            let Some(speaking) = event.arguments.get(1).and_then(Value::as_bool) else {
                return false;
            };
            user.is_speaking = speaking;
        }
        "UserStartedStreaming" if first.as_str() == Some(&user.id) => {
            let Some(quality) = event.arguments.get(1).and_then(Value::as_str) else {
                return false;
            };
            user.is_streaming = true;
            user.stream_quality = Some(quality.into());
            user.allow_remote_control = quality == "medium" || user.allow_remote_control;
        }
        "UserStoppedStreaming" if first.as_str() == Some(&user.id) => {
            user.is_streaming = false;
            user.stream_quality = None;
            user.allow_remote_control = false;
        }
        _ => return false,
    }
    true
}

pub fn apply_event(data: &mut Bootstrap, event: &HubEvent) -> bool {
    let first = event.arguments.first().unwrap_or(&Value::Null);
    match event.target.as_str() {
        "SyncFullChannelState" => {
            let Ok(mut channels) =
                serde_json::from_value::<BTreeMap<String, Vec<User>>>(first.clone())
            else {
                return false;
            };
            let mut members = BTreeSet::new();
            if channels.iter().any(|(channel, users)| {
                channel.is_empty()
                    || users
                        .iter()
                        .any(|user| user.id.is_empty() || !members.insert(user.id.clone()))
            }) {
                return false;
            }
            let mut locations = BTreeMap::new();
            for (channel, users) in &mut channels {
                for user in users {
                    user.current_channel_id = Some(channel.clone());
                    user.current_call_user_id = None;
                    user.is_online = true;
                    locations.insert(user.id.clone(), channel.clone());
                }
            }
            for user in std::iter::once(&mut data.user)
                .chain(data.friends.iter_mut())
                .chain(data.requests.iter_mut())
            {
                user.current_channel_id = locations.get(&user.id).cloned();
                if user.current_channel_id.is_some() {
                    user.current_call_user_id = None;
                } else {
                    user.is_speaking = false;
                }
            }
            data.channel_users = channels;
        }
        "ChannelCreated" | "ChannelUpdated" => {
            let Ok(channel) = serde_json::from_value::<Channel>(first.clone()) else {
                return false;
            };
            if channel.id.is_empty() {
                return false;
            }
            if let Some(existing) = data.channels.iter_mut().find(|item| item.id == channel.id) {
                *existing = channel;
            } else if event.target == "ChannelCreated" {
                data.channels.push(channel);
            }
        }
        "ChannelDeleted" => {
            let Some(channel) = first.as_str().filter(|id| !id.is_empty()) else {
                return false;
            };
            data.channels.retain(|item| item.id != channel);
            data.channel_users.remove(channel);
            data.invites.retain(|invite| invite.channel_id != channel);
            visit_users(data, |user, _| {
                if user.current_channel_id.as_deref() == Some(channel) {
                    user.current_channel_id = None;
                    user.is_speaking = false;
                }
            });
        }
        "UserJoinedChannel" => {
            let Ok(mut user) = serde_json::from_value::<User>(first.clone()) else {
                return false;
            };
            let Some(channel) = event
                .arguments
                .get(1)
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                return false;
            };
            if user.id.is_empty() {
                return false;
            }
            user.current_channel_id = Some(channel.into());
            user.current_call_user_id = None;
            user.is_online = true;
            visit_users(data, |existing, _| {
                if existing.id == user.id {
                    *existing = user.clone();
                }
            });
            remove_member(data, &user.id, None);
            data.channel_users
                .entry(channel.into())
                .or_default()
                .push(user);
        }
        "UserLeftChannel" => {
            let (Some(id), Some(channel)) = (
                first.as_str(),
                event.arguments.get(1).and_then(Value::as_str),
            ) else {
                return false;
            };
            if id.is_empty() || channel.is_empty() {
                return false;
            }
            remove_member(data, id, Some(channel));
            visit_users(data, |user, _| {
                if user.id == id && user.current_channel_id.as_deref() == Some(channel) {
                    user.current_channel_id = None;
                    user.is_speaking = false;
                }
            });
        }
        "FriendRequestReceived" | "FriendRequestAccepted" | "FriendAdded" => {
            let Ok(user) = serde_json::from_value::<User>(first.clone()) else {
                return false;
            };
            if user.id.is_empty() || user.id == data.user.id {
                return false;
            }
            if event.target == "FriendRequestReceived" {
                if !data.friends.iter().any(|friend| friend.id == user.id) {
                    upsert_user(&mut data.requests, user);
                }
            } else {
                data.requests.retain(|request| request.id != user.id);
                upsert_user(&mut data.friends, user);
            }
        }
        "FriendRemoved" | "UserDeleted" => {
            let Some(id) = first.as_str() else {
                return false;
            };
            data.friends.retain(|friend| friend.id != id);
            if event.target == "UserDeleted" {
                data.requests.retain(|request| request.id != id);
                data.invites.retain(|invite| invite.sender_id != id);
                remove_member(data, id, None);
            }
        }
        "ReceiveChannelInvite" => {
            let Ok(invite) = serde_json::from_value::<ChannelInvite>(first.clone()) else {
                return false;
            };
            if invite.channel_id.is_empty() || invite.sender_id.is_empty() {
                return false;
            }
            if !data
                .invites
                .iter()
                .any(|existing| existing.channel_id == invite.channel_id)
            {
                data.invites.push(invite);
            }
        }
        _ => {
            let mut changed = false;
            visit_users(data, |user, local| {
                changed |= apply_user_event(user, event, local)
            });
            if event.target == "UserLeft" {
                if let Some(id) = first.as_str() {
                    remove_member(data, id, None);
                    changed = true;
                }
            }
            return changed;
        }
    }
    true
}

fn visit_users(data: &mut Bootstrap, mut visit: impl FnMut(&mut User, bool)) {
    let local_id = data.user.id.clone();
    visit(&mut data.user, true);
    for user in data
        .friends
        .iter_mut()
        .chain(data.requests.iter_mut())
        .chain(data.channel_users.values_mut().flatten())
        .chain(data.call_user.iter_mut())
    {
        let local = user.id == local_id;
        visit(user, local);
    }
}

fn remove_member(data: &mut Bootstrap, id: &str, channel: Option<&str>) {
    for (key, users) in &mut data.channel_users {
        if channel.is_none_or(|channel| key == channel) {
            users.retain(|user| user.id != id);
        }
    }
}

fn upsert_user(users: &mut Vec<User>, user: User) {
    if let Some(existing) = users.iter_mut().find(|existing| existing.id == user.id) {
        *existing = user;
    } else {
        users.push(user);
    }
}
