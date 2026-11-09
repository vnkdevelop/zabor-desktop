use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use zabor_native::auth::{AuthError, User};
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubEvent;

const CACHE_TTL: Duration = Duration::from_secs(15);
const CACHE_ENTRIES: usize = 100;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const PENDING_REQUESTS: usize = 8;

#[derive(Clone)]
pub(super) struct Request {
    identity: Arc<()>,
    pub(super) ticket: IntentTicket,
    pub(super) stop: CancellationToken,
    pub(super) owner: String,
    pub(super) id: String,
    pub(super) username: String,
    key: String,
}

impl Request {
    pub(super) fn new(
        owner: &str,
        id: &str,
        username: &str,
        intents: &CriticalIntents,
    ) -> Option<Self> {
        let username = trimmed(username);
        if owner.is_empty()
            || owner.len() > 4096
            || id.is_empty()
            || id.len() > 4096
            || username.is_empty()
            || username.encode_utf16().count() > 25
        {
            return None;
        }
        let ticket = intents.ticket();
        let stop = intents.session_token(ticket);
        if stop.is_cancelled() {
            return None;
        }
        Some(Self {
            identity: Arc::new(()),
            ticket,
            stop,
            owner: owner.into(),
            id: id.into(),
            key: username.to_lowercase(),
            username: username.into(),
        })
    }

    pub(super) fn alive(&self) -> bool {
        !self.stop.is_cancelled()
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    pub(super) fn matches_user(&self, user: &User) -> bool {
        user.id == self.id && key(&user.username) == self.key
    }

    pub(super) fn matches_profile(&self, owner: &str, id: &str, username: &str) -> bool {
        self.owner == owner && self.id == id && self.key == key(username)
    }
}

pub(super) enum Lookup {
    Cached(Arc<User>),
    Pending(Request),
    Start(Request),
}

struct Cached {
    user: Arc<User>,
    expires: Instant,
    bytes: usize,
}

struct Pending {
    request: Request,
    invalidated: bool,
}

pub(super) struct Cache {
    ticket: Option<IntentTicket>,
    owner: String,
    users: BTreeMap<String, Cached>,
    order: VecDeque<String>,
    pending: BTreeMap<String, Pending>,
    bytes: usize,
    maximum_bytes: usize,
}

impl Cache {
    pub(super) fn new() -> Self {
        Self {
            ticket: None,
            owner: String::new(),
            users: BTreeMap::new(),
            order: VecDeque::new(),
            pending: BTreeMap::new(),
            bytes: 0,
            maximum_bytes: CACHE_BYTES,
        }
    }

    pub(super) fn clear(&mut self) {
        for pending in self.pending.values() {
            pending.request.stop.cancel();
        }
        self.pending.clear();
        self.users.clear();
        self.order.clear();
        self.bytes = 0;
        self.ticket = None;
        self.owner.clear();
    }

    pub(super) fn sync(&mut self, owner: &str, intents: &CriticalIntents) {
        if self.owner != owner
            || self
                .ticket
                .is_some_and(|ticket| !intents.is_current(ticket, IntentScope::Session))
        {
            self.clear();
        }
        if !owner.is_empty() && self.ticket.is_none() {
            self.owner = owner.into();
            self.ticket = Some(intents.ticket());
        }
    }

    pub(super) fn is_current(&self, owner: &str, intents: &CriticalIntents) -> bool {
        self.owner == owner
            && self
                .ticket
                .is_some_and(|ticket| intents.is_current(ticket, IntentScope::Session))
    }

    pub(super) fn prepare(
        &mut self,
        owner: &str,
        user: &User,
        intents: &CriticalIntents,
        now: Instant,
    ) -> Option<Lookup> {
        self.sync(owner, intents);
        let request = Request::new(owner, &user.id, &user.username, intents)?;
        if request.id == request.owner {
            return None;
        }
        if let Some(cached) = self.users.get(&request.key) {
            if cached.expires > now && request.matches_user(&cached.user) {
                return Some(Lookup::Cached(cached.user.clone()));
            }
            self.remove(&request.key);
        }
        if let Some(pending) = self.pending.get(&request.key) {
            return (pending.request.alive() && pending.request.id == user.id)
                .then(|| Lookup::Pending(pending.request.clone()));
        }
        if self.pending.len() >= PENDING_REQUESTS {
            return None;
        }
        self.pending.insert(
            request.key.clone(),
            Pending {
                request: request.clone(),
                invalidated: false,
            },
        );
        Some(Lookup::Start(request))
    }

    pub(super) fn finish(
        &mut self,
        request: &Request,
        result: Result<Option<User>, AuthError>,
        intents: &CriticalIntents,
        now: Instant,
    ) -> Option<Arc<User>> {
        if !self
            .pending
            .get(&request.key)
            .is_some_and(|pending| pending.request.same(request))
        {
            return None;
        }
        let pending = self.pending.remove(&request.key)?;
        if pending.invalidated || !request.alive() || self.owner != request.owner {
            return None;
        }
        let user = result.ok()??;
        if !request.matches_user(&user) {
            return None;
        }
        let bytes = serde_json::to_vec(&user).ok()?.len();
        if bytes > zabor_native::avatar::MAX_WIRE_BYTES || bytes > self.maximum_bytes {
            return None;
        }
        let user = Arc::new(user);
        intents.commit(request.ticket, IntentScope::Session, || {
            self.remove(&request.key);
            while self.users.len() >= CACHE_ENTRIES || self.bytes + bytes > self.maximum_bytes {
                let oldest = self.order.front()?.clone();
                self.remove(&oldest);
            }
            self.bytes += bytes;
            self.order.push_back(request.key.clone());
            self.users.insert(
                request.key.clone(),
                Cached {
                    user: user.clone(),
                    expires: now + CACHE_TTL,
                    bytes,
                },
            );
            Some(user)
        })?
    }

    pub(super) fn observe(&mut self, event: &HubEvent) {
        let first = event.arguments.first();
        let (id, username) = match event.target.as_str() {
            "UserJoined" | "UserUpdated" => {
                let Some(first) = first else { return };
                (first["id"].as_str(), first["username"].as_str())
            }
            "UserLeft" | "UserDeleted" => (first.and_then(serde_json::Value::as_str), None),
            _ => return,
        };
        let username = username.map(key);
        let remove: Vec<_> = self
            .users
            .iter()
            .filter(|(key, cached)| {
                id == Some(cached.user.id.as_str()) || username.as_ref() == Some(*key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in remove {
            self.remove(&key);
        }
        for (key, pending) in &mut self.pending {
            if id == Some(pending.request.id.as_str()) || username.as_ref() == Some(key) {
                pending.invalidated = true;
            }
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(cached) = self.users.remove(key) {
            self.bytes -= cached.bytes;
        }
        self.order.retain(|entry| entry != key);
    }
}

fn key(username: &str) -> String {
    trimmed(username).to_lowercase()
}

fn trimmed(value: &str) -> &str {
    value.trim_matches(|character| {
        matches!(character, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zabor_native::critical_intents::CriticalAction;

    fn user(id: &str) -> User {
        User {
            id: id.into(),
            username: id.into(),
            display_name: "fresh".into(),
            ..Default::default()
        }
    }

    fn start(cache: &mut Cache, user: &User, intents: &CriticalIntents, now: Instant) -> Request {
        let Some(Lookup::Start(request)) = cache.prepare("owner", user, intents, now) else {
            panic!("lookup did not start")
        };
        request
    }

    #[test]
    fn case_trim_keys_deduplicate_original_pending_and_expire_at_fifteen_seconds() {
        let intents = CriticalIntents::default();
        let mut cache = Cache::new();
        let now = Instant::now();
        let mut selected = user("peer");
        selected.username = "\u{feff} PEER \u{00a0}".into();
        let request = start(&mut cache, &selected, &intents, now);
        assert_eq!(request.username, "PEER");
        let Some(Lookup::Pending(pending)) = cache.prepare("owner", &user("peer"), &intents, now)
        else {
            panic!("pending was not deduplicated")
        };
        assert!(request.same(&pending));
        assert!(cache
            .finish(&request, Ok(Some(user("peer"))), &intents, now)
            .is_some());
        assert!(matches!(
            cache.prepare(
                "owner",
                &selected,
                &intents,
                now + CACHE_TTL - Duration::from_nanos(1)
            ),
            Some(Lookup::Cached(_))
        ));
        assert!(matches!(
            cache.prepare("owner", &selected, &intents, now + CACHE_TTL),
            Some(Lookup::Start(_))
        ));
        assert_eq!(key("\u{0085}name\u{0085}"), "\u{0085}name\u{0085}");
    }

    #[test]
    fn cache_is_fifo_at_one_hundred_entries_and_bounded_by_retained_bytes() {
        let intents = CriticalIntents::default();
        let mut cache = Cache::new();
        let now = Instant::now();
        for index in 0..=CACHE_ENTRIES {
            let user = user(&format!("peer-{index}"));
            let request = start(&mut cache, &user, &intents, now);
            assert!(cache
                .finish(&request, Ok(Some(user)), &intents, now)
                .is_some());
        }
        assert_eq!(cache.users.len(), CACHE_ENTRIES);
        assert!(!cache.users.contains_key("peer-0"));
        assert!(cache.users.contains_key("peer-1"));
        assert_eq!(
            cache.bytes,
            cache.users.values().map(|user| user.bytes).sum::<usize>()
        );
        cache.clear();
        cache.maximum_bytes = serde_json::to_vec(&user("first")).unwrap().len() + 2;
        for id in ["first", "other"] {
            let selected = user(id);
            let request = start(&mut cache, &selected, &intents, now);
            assert!(cache
                .finish(&request, Ok(Some(selected)), &intents, now)
                .is_some());
        }
        assert_eq!(cache.users.len(), 1);
        assert!(cache.users.contains_key("other"));
        assert!(cache.bytes <= cache.maximum_bytes);
    }

    #[tokio::test]
    async fn leave_preserves_original_request_but_logout_or_owner_replacement_never_refills_cache()
    {
        let intents = CriticalIntents::default();
        let now = Instant::now();
        let mut cache = Cache::new();
        let request = start(&mut cache, &user("peer"), &intents, now);
        intents.request(CriticalAction::Leave);
        assert!(request.alive());
        assert!(cache
            .finish(&request, Ok(Some(user("peer"))), &intents, now)
            .is_some());
        assert_eq!(intents.next().await, CriticalAction::Leave);
        let request = start(&mut cache, &user("next"), &intents, now);
        intents.request(CriticalAction::Logout);
        cache.sync("owner", &intents);
        assert!(!request.alive());
        assert!(cache
            .finish(&request, Ok(Some(user("next"))), &intents, now)
            .is_none());
        assert!(cache.users.is_empty());
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let request = start(&mut cache, &user("last"), &intents, now);
        cache.sync("new-owner", &intents);
        assert!(!request.alive());
        assert!(cache
            .finish(&request, Ok(Some(user("last"))), &intents, now)
            .is_none());
    }

    #[test]
    fn original_identity_rejects_late_replacement_error_null_or_mismatched_user() {
        let intents = CriticalIntents::default();
        let mut cache = Cache::new();
        let now = Instant::now();
        let request = start(&mut cache, &user("peer"), &intents, now);
        let imposter = Request::new("owner", "peer", "peer", &intents).unwrap();
        assert!(cache
            .finish(&imposter, Ok(Some(user("peer"))), &intents, now)
            .is_none());
        assert_eq!(cache.pending.len(), 1);
        assert!(cache
            .finish(&request, Ok(Some(user("other"))), &intents, now)
            .is_none());
        let request = start(&mut cache, &user("peer"), &intents, now);
        assert!(cache
            .finish(&request, Err(AuthError::Network), &intents, now)
            .is_none());
        let request = start(&mut cache, &user("peer"), &intents, now);
        assert!(cache.finish(&request, Ok(None), &intents, now).is_none());
        assert!(cache.users.is_empty() && cache.pending.is_empty());
    }

    #[test]
    fn joined_updated_left_and_deleted_invalidate_cached_and_earlier_pending_profiles() {
        let intents = CriticalIntents::default();
        let now = Instant::now();
        for (target, arguments) in [
            ("UserJoined", vec![json!(user("peer"))]),
            ("UserUpdated", vec![json!(user("peer"))]),
            ("UserLeft", vec![json!("peer")]),
            ("UserDeleted", vec![json!("peer")]),
        ] {
            let mut cache = Cache::new();
            let request = start(&mut cache, &user("peer"), &intents, now);
            cache.observe(&HubEvent {
                target: target.into(),
                arguments: arguments.clone(),
            });
            assert!(cache
                .finish(&request, Ok(Some(user("peer"))), &intents, now)
                .is_none());
            let request = start(&mut cache, &user("peer"), &intents, now);
            assert!(cache
                .finish(&request, Ok(Some(user("peer"))), &intents, now)
                .is_some());
            cache.observe(&HubEvent {
                target: target.into(),
                arguments,
            });
            assert!(cache.users.is_empty());
        }
    }

    #[test]
    fn self_lookup_and_pending_overflow_never_start_more_than_eight_operations() {
        let intents = CriticalIntents::default();
        let mut cache = Cache::new();
        let now = Instant::now();
        assert!(cache
            .prepare("owner", &user("owner"), &intents, now)
            .is_none());
        for index in 0..PENDING_REQUESTS {
            start(&mut cache, &user(&format!("peer-{index}")), &intents, now);
        }
        assert!(cache
            .prepare("owner", &user("overflow"), &intents, now)
            .is_none());
        assert_eq!(cache.pending.len(), PENDING_REQUESTS);
    }
}
