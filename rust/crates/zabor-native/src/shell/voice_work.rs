use super::{HubConnection, MediaEvent, UiEvent, VideoReceiver, VoiceSession};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubError;
use zabor_native::media::PeerLifetime;

type VoiceResources = (Option<VoiceSession>, Option<mpsc::Receiver<MediaEvent>>);
type VoiceMediaResources = (
    Option<VoiceSession>,
    Option<mpsc::Receiver<MediaEvent>>,
    BTreeMap<String, VideoReceiver>,
);

pub(super) struct VoiceWork<'a> {
    intents: &'a CriticalIntents,
    ticket: IntentTicket,
    stop: &'a CancellationToken,
    update: &'a (dyn Fn(UiEvent) + Send + Sync),
}

impl<'a> VoiceWork<'a> {
    pub(super) fn new(
        intents: &'a CriticalIntents,
        ticket: IntentTicket,
        stop: &'a CancellationToken,
        update: &'a (dyn Fn(UiEvent) + Send + Sync),
    ) -> Self {
        Self {
            intents,
            ticket,
            stop,
            update,
        }
    }

    pub(super) fn is_current(&self) -> bool {
        !self.stop.is_cancelled() && self.intents.is_current(self.ticket, IntentScope::Voice)
    }

    pub(super) fn commit<T>(&self, commit: impl FnOnce() -> T) -> Option<T> {
        self.intents
            .commit(self.ticket, IntentScope::Voice, || {
                (!self.stop.is_cancelled()).then(commit)
            })
            .flatten()
    }

    pub(super) fn commit_resource<T>(
        &self,
        resource_ticket: Option<IntentTicket>,
        mut accepted: impl FnMut() -> T,
    ) -> Option<T> {
        self.commit(|| (resource_ticket == Some(self.ticket)).then(&mut accepted))
            .flatten()
    }

    pub(super) fn take_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: Option<IntentTicket>,
        mut accepted: impl FnMut(Option<&VoiceSession>),
    ) -> Option<VoiceResources> {
        let mut resources = None;
        let taken = self.commit(|| {
            if resource_ticket != Some(self.ticket) {
                return false;
            }
            resources = Some((voice.take(), events.take()));
            accepted(
                resources
                    .as_ref()
                    .and_then(|resources| resources.0.as_ref()),
            );
            true
        });
        taken.filter(|taken| *taken).and(resources)
    }

    pub(super) fn commit_peer_resource<T>(
        &self,
        resource_ticket: Option<IntentTicket>,
        lifetime: &PeerLifetime,
        mut accepted: impl FnMut() -> T,
    ) -> Option<T> {
        self.commit_resource(resource_ticket, || {
            lifetime.commit(|| (!self.stop.is_cancelled()).then(&mut accepted))
        })
        .flatten()
        .flatten()
    }

    pub(super) fn install_video(
        &self,
        videos: &mut BTreeMap<String, VideoReceiver>,
        resource_ticket: Option<IntentTicket>,
        lifetime: &PeerLifetime,
        prepared: (String, VideoReceiver),
        mut accepted: impl FnMut(),
    ) -> Option<Option<VideoReceiver>> {
        let mut prepared = Some(prepared);
        let mut replaced = None;
        let installed = self.commit_peer_resource(resource_ticket, lifetime, || {
            if !prepared.as_ref().unwrap().1.lifetime().is_same(lifetime) {
                return false;
            }
            let (peer, receiver) = prepared.take().unwrap();
            replaced = Some(videos.insert(peer, receiver));
            accepted();
            true
        });
        installed.filter(|installed| *installed).and(replaced)
    }

    pub(super) fn take_video(
        &self,
        videos: &mut BTreeMap<String, VideoReceiver>,
        resource_ticket: Option<IntentTicket>,
        lifetime: Option<&PeerLifetime>,
        peer: &str,
        mut accepted: impl FnMut(&BTreeMap<String, VideoReceiver>) -> bool,
    ) -> Option<Option<VideoReceiver>> {
        let mut receiver = None;
        let mut take = || {
            if videos.get(peer).is_some_and(|receiver| {
                lifetime.is_none_or(|lifetime| !receiver.lifetime().is_same(lifetime))
            }) {
                return false;
            }
            if accepted(videos) {
                receiver = videos.remove(peer);
            }
            true
        };
        let taken = match lifetime {
            Some(lifetime) => self.commit_peer_resource(resource_ticket, lifetime, &mut take),
            None => self.commit_resource(resource_ticket, &mut take),
        };
        taken.filter(|taken| *taken).map(|_| receiver)
    }

    pub(super) fn retire_videos(
        &self,
        videos: &mut BTreeMap<String, VideoReceiver>,
        resource_ticket: Option<IntentTicket>,
        mut selected: impl FnMut(&str, &VideoReceiver) -> bool,
    ) -> Option<BTreeMap<String, VideoReceiver>> {
        let mut retired = BTreeMap::new();
        let accepted = self.commit_resource(resource_ticket, || {
            for peer in videos.keys().cloned().collect::<Vec<_>>() {
                if self.stop.is_cancelled() {
                    break;
                }
                let receiver = videos.get(&peer).unwrap();
                if !selected(&peer, receiver) {
                    continue;
                }
                let lifetime = receiver.lifetime().clone();
                let mut take = || {
                    if !self.stop.is_cancelled() {
                        retired.insert(peer.clone(), videos.remove(&peer).unwrap());
                    }
                };
                if lifetime.is_alive() {
                    lifetime.commit(&mut take);
                } else {
                    take();
                }
            }
        });
        accepted.map(|_| retired)
    }

    pub(super) fn take_previous_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: Option<IntentTicket>,
        retain_events: bool,
        mut accepted: impl FnMut(Option<&VoiceSession>),
    ) -> Option<VoiceResources> {
        let mut resources = None;
        let taken = self.commit(|| {
            if (voice.is_some() || events.is_some()) && resource_ticket != Some(self.ticket) {
                return false;
            }
            resources = Some((
                voice.take(),
                if retain_events { None } else { events.take() },
            ));
            accepted(
                resources
                    .as_ref()
                    .and_then(|resources| resources.0.as_ref()),
            );
            true
        });
        taken.filter(|taken| *taken).and(resources)
    }

    pub(super) fn take_previous_media(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        videos: &mut BTreeMap<String, VideoReceiver>,
        resource_ticket: Option<IntentTicket>,
        retain_events: bool,
        mut accepted: impl FnMut(Option<&VoiceSession>),
    ) -> Option<VoiceMediaResources> {
        let mut resources = None;
        let taken = self.commit(|| {
            if (voice.is_some() || events.is_some() || !videos.is_empty())
                && resource_ticket != Some(self.ticket)
            {
                return false;
            }
            if voice.is_none() && videos.values().any(VideoReceiver::is_alive) {
                return false;
            }
            resources = Some((
                voice.take(),
                if retain_events { None } else { events.take() },
                std::mem::take(videos),
            ));
            accepted(
                resources
                    .as_ref()
                    .and_then(|resources| resources.0.as_ref()),
            );
            true
        });
        taken.filter(|taken| *taken).and(resources)
    }

    pub(super) fn install_previous_media(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: Option<IntentTicket>,
        mut prepared: VoiceMediaResources,
        mut accepted: impl FnMut(),
    ) -> Option<VoiceMediaResources> {
        let installed = self.commit_resource(resource_ticket, || {
            if prepared.2.values().any(VideoReceiver::is_alive) {
                return false;
            }
            std::mem::swap(voice, &mut prepared.0);
            std::mem::swap(events, &mut prepared.1);
            accepted();
            true
        });
        installed.filter(|installed| *installed).map(|_| prepared)
    }

    pub(super) fn install_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: &mut Option<IntentTicket>,
        prepared: (VoiceSession, mpsc::Receiver<MediaEvent>),
        mut accepted: impl FnMut(),
    ) -> Option<VoiceResources> {
        let mut prepared = Some(prepared);
        let mut replaced = None;
        let installed = self.commit(|| {
            let (session, receiver) = prepared.take().unwrap();
            replaced = Some((voice.replace(session), events.replace(receiver)));
            *resource_ticket = Some(self.ticket);
            accepted();
            true
        });
        installed.filter(|installed| *installed).and(replaced)
    }

    pub(super) fn publish(&self, event: UiEvent) {
        if self.is_current() {
            (self.update)(event.guarded(self.ticket, IntentScope::Voice));
        }
    }

    pub(super) fn publish_peer(&self, lifetime: &PeerLifetime, event: UiEvent) {
        if lifetime.is_alive() {
            self.publish(UiEvent::PeerGuarded(lifetime.clone(), Box::new(event)));
        }
    }

    pub(super) async fn run<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => None,
            result = self.intents.run(self.ticket, IntentScope::Voice, future) => result.filter(|_| self.is_current()),
        }
    }

    pub(super) async fn run_peer<T>(
        &self,
        lifetime: &PeerLifetime,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        if !lifetime.is_alive() {
            return None;
        }
        tokio::select! {
            biased;
            _ = lifetime.cancelled() => None,
            result = self.run(future) => result.filter(|_| lifetime.is_alive() && self.is_current()),
        }
    }

    pub(super) async fn send_peer(
        &self,
        lifetime: &PeerLifetime,
        hub: &HubConnection,
        target: &str,
        arguments: Vec<Value>,
    ) -> Option<Result<(), HubError>> {
        self.run_peer(lifetime, hub.send(target, arguments)).await
    }
}
