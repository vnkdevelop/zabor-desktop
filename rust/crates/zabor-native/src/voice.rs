use crate::auth::User;
use crate::hub::HubConnection;
use crate::media::{IceServerSettings, MediaEngine, MediaError, MediaEvent};
use crate::screen::{ScreenCapture, ScreenSource};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Deserialize)]
pub struct ChannelMembership {
    pub users: Vec<User>,
}

pub struct VoiceSession {
    engine: MediaEngine,
    local_id: String,
    peers: BTreeSet<String>,
    pub channel: Option<String>,
    pub call_peer: Option<String>,
    muted: bool,
    deafened: bool,
    server_muted: bool,
    server_deafened: bool,
    relay_only: bool,
    capture: Option<ScreenCapture>,
}

impl VoiceSession {
    pub async fn new(
        hub: &HubConnection,
        user: &User,
        relay_only: bool,
    ) -> Result<(Self, mpsc::Receiver<MediaEvent>), MediaError> {
        let local_id = user.id.clone();
        let (mut engine, events) = MediaEngine::new(local_id.clone());
        let settings = hub
            .invoke("GetIceServers", vec![], Duration::from_secs(5))
            .await
            .map_err(|_| MediaError::Backend)?;
        let servers: Vec<IceServerSettings> =
            serde_json::from_value(settings["iceServers"].clone())
                .map_err(|_| MediaError::Backend)?;
        if relay_only
            && !servers.iter().any(|server| {
                server
                    .urls
                    .iter()
                    .any(|url| url.starts_with("turn:") || url.starts_with("turns:"))
            })
        {
            return Err(MediaError::Backend);
        }
        engine.configure_ice(servers, relay_only)?;
        Ok((
            Self {
                engine,
                local_id,
                peers: BTreeSet::new(),
                channel: None,
                call_peer: None,
                muted: user.is_muted,
                deafened: user.is_deafened,
                server_muted: user.is_server_muted,
                server_deafened: user.is_server_deafened,
                relay_only,
                capture: None,
            },
            events,
        ))
    }

    pub async fn join(
        &mut self,
        hub: &HubConnection,
        channel: &str,
    ) -> Result<Vec<User>, MediaError> {
        self.engine.start_microphone()?;
        self.engine
            .set_muted(self.muted || self.deafened || self.server_muted || self.server_deafened);
        self.engine
            .set_deafened(self.deafened || self.server_deafened);
        let value = hub
            .invoke(
                "JoinChannel",
                vec![json!({"channelId":channel})],
                Duration::from_secs(20),
            )
            .await
            .map_err(|_| MediaError::Backend)?;
        if value.is_null() {
            return Err(MediaError::ChannelFull);
        }
        let membership: ChannelMembership =
            serde_json::from_value(value).map_err(|_| MediaError::Backend)?;
        self.channel = Some(channel.to_owned());
        self.call_peer = None;
        let previous = self.peers.clone();
        self.peers = membership
            .users
            .iter()
            .filter(|user| user.id != self.local_id)
            .map(|user| user.id.clone())
            .collect();
        self.engine.authorize_peers(self.peers.iter().cloned());
        for id in self.peers.iter().filter(|id| !previous.contains(*id)) {
            let offer = self.engine.offer(id, false).await?;
            hub.send("SendWebRTCOffer", vec![json!(id), json!(offer)])
                .await
                .map_err(|_| MediaError::Backend)?;
        }
        Ok(membership.users)
    }

    pub async fn leave(&mut self, hub: &HubConnection) {
        let was_call = self.call_peer.take().is_some();
        let was_sharing = self.capture.is_some();
        self.capture = None;
        self.engine.close();
        self.peers.clear();
        self.channel = None;
        if was_sharing {
            let _ = hub.send("StopStream", Vec::new()).await;
        }
        let _ = hub
            .invoke(
                if was_call { "EndCall" } else { "LeaveChannel" },
                vec![],
                Duration::from_secs(5),
            )
            .await;
    }

    pub async fn rejoin(&mut self, hub: &HubConnection) -> Result<Vec<User>, MediaError> {
        let channel = self.channel.clone().ok_or(MediaError::Backend)?;
        let settings = hub
            .invoke("GetIceServers", Vec::new(), Duration::from_secs(5))
            .await
            .map_err(|_| MediaError::Backend)?;
        let servers = serde_json::from_value(settings["iceServers"].clone())
            .map_err(|_| MediaError::Backend)?;
        self.engine.configure_ice(servers, self.relay_only)?;
        self.peers.clear();
        self.engine.authorize_peers(Vec::<String>::new());
        self.join(hub, &channel).await
    }

    pub fn prepare_call(&mut self, peer: &str) -> Result<(), MediaError> {
        self.engine.start_microphone()?;
        self.engine
            .set_muted(self.muted || self.deafened || self.server_muted || self.server_deafened);
        self.engine
            .set_deafened(self.deafened || self.server_deafened);
        self.channel = None;
        self.call_peer = Some(peer.into());
        self.peers = [peer.to_owned()].into_iter().collect();
        self.engine.authorize_peers(self.peers.iter().cloned());
        Ok(())
    }

    pub async fn connect_call(
        &mut self,
        hub: &HubConnection,
        peer: &str,
    ) -> Result<(), MediaError> {
        if self.call_peer.as_deref() != Some(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        let offer = self.engine.offer(peer, false).await?;
        hub.send("SendWebRTCOffer", vec![json!(peer), json!(offer)])
            .await
            .map_err(|_| MediaError::Backend)
    }

    pub async fn renegotiate(&mut self, hub: &HubConnection, peer: &str) -> Result<(), MediaError> {
        if !self.peers.contains(peer) {
            return Ok(());
        }
        if let Some(offer) = self.engine.offer_when_stable(peer).await? {
            hub.send("SendWebRTCOffer", vec![json!(peer), json!(offer)])
                .await
                .map_err(|_| MediaError::Backend)?;
        }
        Ok(())
    }

    pub async fn start_share(
        &mut self,
        hub: &HubConnection,
        source: ScreenSource,
        quality: &str,
    ) -> Result<(), MediaError> {
        let (width, height, fps) = match quality {
            "low" => (1280, 720, 30),
            "high" => (1920, 1080, 60),
            _ => return Err(MediaError::Backend),
        };
        let capture = tokio::task::spawn_blocking(move || {
            ScreenCapture::start(source, width, height, fps, true)
        })
        .await
        .map_err(|_| MediaError::Backend)??;
        let started = hub
            .invoke("StartStream", vec![json!(quality)], Duration::from_secs(10))
            .await
            .map_err(|_| MediaError::Backend)?;
        if started != true {
            return Err(MediaError::Backend);
        }
        if let Err(error) = self.engine.set_video(Some(capture.source.clone())) {
            let _ = self.engine.set_video(None);
            let _ = hub.send("StopStream", Vec::new()).await;
            return Err(error);
        }
        self.capture = Some(capture);
        for peer in self.peers.clone() {
            self.renegotiate(hub, &peer).await?;
        }
        Ok(())
    }

    pub async fn stop_share(&mut self, hub: &HubConnection) {
        self.capture = None;
        let _ = self.engine.set_video(None);
        let _ = hub.send("StopStream", Vec::new()).await;
        for peer in self.peers.clone() {
            let _ = self.renegotiate(hub, &peer).await;
        }
    }

    pub fn share_failed(&self) -> bool {
        self.capture.as_ref().is_some_and(ScreenCapture::failed)
    }
    pub fn is_sharing(&self) -> bool {
        self.capture.is_some()
    }

    pub fn has_peer(&self, peer: &str) -> bool {
        self.peers.contains(peer)
    }

    pub async fn mute(
        &mut self,
        hub: &HubConnection,
        muted: bool,
        deafened: bool,
    ) -> Result<(), MediaError> {
        self.muted = muted;
        self.deafened = deafened;
        self.engine
            .set_muted(muted || deafened || self.server_muted || self.server_deafened);
        self.engine.set_deafened(deafened || self.server_deafened);
        hub.send(
            "UpdateUserState",
            vec![json!({"userId":self.local_id,"isMuted":muted,"isDeafened":deafened})],
        )
        .await
        .map_err(|_| MediaError::Backend)
    }

    pub async fn handle(
        &mut self,
        hub: &HubConnection,
        target: &str,
        args: &[Value],
    ) -> Result<(), MediaError> {
        let peer = args.first().and_then(Value::as_str);
        let payload = args.get(1).and_then(Value::as_str);
        match target {
            "UserStateChanged" | "UserUpdated" => {
                if let Some(user) = args.first() {
                    if user["userId"]
                        .as_str()
                        .or_else(|| user["UserId"].as_str())
                        .or_else(|| user["id"].as_str())
                        == Some(self.local_id.as_str())
                    {
                        if let Some(value) = user["isServerMuted"]
                            .as_bool()
                            .or_else(|| user["IsServerMuted"].as_bool())
                        {
                            self.server_muted = value;
                        }
                        if let Some(value) = user["isServerDeafened"]
                            .as_bool()
                            .or_else(|| user["IsServerDeafened"].as_bool())
                        {
                            self.server_deafened = value;
                        }
                        self.engine.set_muted(
                            self.muted
                                || self.deafened
                                || self.server_muted
                                || self.server_deafened,
                        );
                        self.engine
                            .set_deafened(self.deafened || self.server_deafened);
                    }
                }
            }
            "ChannelDeleted" if self.channel.is_some() && peer == self.channel.as_deref() => {
                self.capture = None;
                self.engine.close();
                self.peers.clear();
                self.channel = None;
            }
            "ReceiveWebRTCOffer" | "ReceiveWebRTCAnswer" | "ReceiveIceCandidate" => {
                let (Some(peer), Some(payload)) = (peer, payload) else {
                    return Err(MediaError::InvalidSignal);
                };
                if payload.starts_with("zabor-chat:") {
                    return Ok(());
                }
                if !self.peers.contains(peer) {
                    return Ok(());
                }
                match target {
                    "ReceiveWebRTCOffer" => {
                        if let Some(answer) = self.engine.receive_offer(peer, payload).await? {
                            hub.send("SendWebRTCAnswer", vec![json!(peer), json!(answer)])
                                .await
                                .map_err(|_| MediaError::Backend)?;
                        }
                    }
                    "ReceiveWebRTCAnswer" => self.engine.receive_answer(peer, payload).await?,
                    _ => self.engine.receive_ice(peer, payload).await?,
                }
            }
            "UserJoinedChannel" => {
                let user: User = serde_json::from_value(args.first().cloned().unwrap_or_default())
                    .map_err(|_| MediaError::InvalidSignal)?;
                if self.channel.is_some()
                    && args.get(1).and_then(Value::as_str) == self.channel.as_deref()
                    && !user.id.is_empty()
                    && user.id != self.local_id
                    && self.peers.insert(user.id.clone())
                {
                    self.engine.authorize_peers(self.peers.iter().cloned());
                    let offer = self.engine.offer(&user.id, false).await?;
                    hub.send("SendWebRTCOffer", vec![json!(user.id), json!(offer)])
                        .await
                        .map_err(|_| MediaError::Backend)?;
                }
            }
            "UserLeftChannel" | "UserLeft" | "UserDeleted" => {
                if let Some(peer) = peer {
                    if target != "UserLeftChannel"
                        || (self.channel.is_some()
                            && args.get(1).and_then(Value::as_str) == self.channel.as_deref())
                    {
                        self.peers.remove(peer);
                        self.engine.authorize_peers(self.peers.iter().cloned());
                    }
                }
            }
            "SyncFullChannelState" => {
                if let Some(channel) = &self.channel {
                    let state: std::collections::BTreeMap<String, Vec<User>> =
                        serde_json::from_value(args.first().cloned().unwrap_or_default())
                            .map_err(|_| MediaError::InvalidSignal)?;
                    let peers: BTreeSet<String> = state
                        .get(channel)
                        .into_iter()
                        .flatten()
                        .filter(|user| !user.id.is_empty() && user.id != self.local_id)
                        .map(|user| user.id.clone())
                        .collect();
                    let added: Vec<String> = peers.difference(&self.peers).cloned().collect();
                    self.peers = peers;
                    self.engine.authorize_peers(self.peers.iter().cloned());
                    for peer in added {
                        self.renegotiate(hub, &peer).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
