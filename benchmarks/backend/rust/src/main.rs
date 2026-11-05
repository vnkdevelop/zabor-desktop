use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{digest, pbkdf2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Read, Write};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    password: String,
    salt: String,
    operation: String,
    engine: String,
    iterations: usize,
    batch_size: usize,
    residents: usize,
    warmups: usize,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct Session {
    id: Arc<str>,
    first: Arc<str>,
    second: Arc<str>,
    expires_at: i64,
}

struct Registry {
    sessions: HashMap<Arc<str>, Arc<Session>>,
    clock: Instant,
}

impl Registry {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            clock: Instant::now(),
        }
    }

    fn now(&self) -> i64 {
        self.clock.elapsed().as_millis() as i64
    }

    fn start(&mut self, first: &Arc<str>, second: &Arc<str>) -> Arc<Session> {
        self.end(first);
        self.end(second);
        let mut encoded = [0_u8; 32];
        let id = Uuid::new_v4();
        let id_text: &str = id.simple().encode_lower(&mut encoded);
        let session = Arc::new(Session {
            id: Arc::from(id_text),
            first: first.clone(),
            second: second.clone(),
            expires_at: 0,
        });
        self.sessions.insert(first.clone(), session.clone());
        self.sessions.insert(second.clone(), session.clone());
        session
    }

    fn get(&self, user: &str) -> Option<&Session> {
        self.sessions.get(user).map(Arc::as_ref)
    }

    fn suspend(&mut self, user: &str) -> Option<Arc<Session>> {
        let current = self.get(user)?;
        let session = Arc::new(Session {
            expires_at: if current.expires_at > 0 {
                current.expires_at
            } else {
                self.now() + 45_000
            },
            ..current.clone()
        });
        self.sessions.insert(session.first.clone(), session.clone());
        self.sessions
            .insert(session.second.clone(), session.clone());
        Some(session)
    }

    fn resume(&mut self, user: &str, id: &str) -> Option<Arc<Session>> {
        let current = self.get(user)?;
        if current.id.as_ref() != id || current.expires_at > 0 && current.expires_at <= self.now() {
            return None;
        }
        let session = Arc::new(Session {
            expires_at: 0,
            ..current.clone()
        });
        self.sessions.insert(session.first.clone(), session.clone());
        self.sessions
            .insert(session.second.clone(), session.clone());
        Some(session)
    }

    fn remove(&mut self, session: &Session) {
        if self.get(&session.first).map(|current| &current.id) == Some(&session.id) {
            self.sessions.remove(&session.first);
        }
        if self.get(&session.second).map(|current| &current.id) == Some(&session.id) {
            self.sessions.remove(&session.second);
        }
    }

    fn end(&mut self, user: &str) -> Option<Arc<Session>> {
        let session = self.sessions.get(user)?.clone();
        self.remove(&session);
        Some(session)
    }

    fn expire(&mut self, now: i64) -> Vec<Arc<Session>> {
        let expired: HashSet<Arc<Session>> = self
            .sessions
            .values()
            .filter(|session| session.expires_at > 0 && session.expires_at <= now)
            .cloned()
            .collect();
        for session in &expired {
            self.remove(session);
        }
        expired.into_iter().collect()
    }
}

fn derive(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut output = [0_u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).unwrap(),
        salt,
        password,
        &mut output,
    );
    output
}

fn verify(password: &[u8], encoded: &str, accept_sha: bool) -> (bool, bool) {
    if let Ok(bytes) = STANDARD.decode(encoded) {
        if bytes.len() == 48 {
            for (iterations, legacy) in [(600_000, false), (100_000, true)] {
                if pbkdf2::verify(
                    pbkdf2::PBKDF2_HMAC_SHA256,
                    NonZeroU32::new(iterations).unwrap(),
                    &bytes[..16],
                    password,
                    &bytes[16..],
                )
                .is_ok()
                {
                    return (true, legacy);
                }
            }
        }
        if accept_sha
            && bytes.len() == 32
            && bool::from(
                bytes
                    .as_slice()
                    .ct_eq(digest::digest(&digest::SHA256, password).as_ref()),
            )
        {
            return (true, true);
        }
    }
    (false, false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultData {
    operation: String,
    engine: String,
    batch_ms: Vec<f64>,
    successes: usize,
    legacy_results: usize,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    reader.by_ref().take(65537).read_line(&mut line)?;
    if line.len() > 65536 {
        return Err("input-size".into());
    }
    let input: Input = serde_json::from_str(&line)?;
    if !(1..=1000).contains(&input.iterations)
        || !(1..=100000).contains(&input.batch_size)
        || !(1..=10000).contains(&input.residents)
        || input.warmups > 20
        || input.password.is_empty()
        || input.password.len() > 1024
        || input.engine != "rust-single-owner"
    {
        return Err("input-range".into());
    }
    let salt = STANDARD.decode(&input.salt)?;
    if salt.len() != 16 {
        return Err("salt-size".into());
    }
    let current = derive(input.password.as_bytes(), &salt, 600_000);
    let legacy = derive(input.password.as_bytes(), &salt, 100_000);
    let sha = digest::digest(&digest::SHA256, input.password.as_bytes());
    let current_hash = STANDARD.encode([salt.as_slice(), &current].concat());
    let legacy_hash = STANDARD.encode([salt.as_slice(), &legacy].concat());
    let sha_hash = STANDARD.encode(sha.as_ref());
    let rejected_password = format!("!{}", input.password);
    let accept_sha = std::env::var("ZABOR_ACCEPT_LEGACY_HASHES")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    let proof = digest::digest(
        &digest::SHA256,
        &[current.as_slice(), legacy.as_slice(), sha.as_ref()].concat(),
    );
    let proof_hex: String = proof
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let users: Vec<Arc<str>> = (0..input.residents * 2)
        .map(|index| Arc::from(format!("u{index:06}")))
        .collect();
    let mut registry = Registry::new();
    for pair in 0..input.residents {
        registry.start(&users[pair * 2], &users[pair * 2 + 1]);
    }
    println!(
        "{}",
        serde_json::json!({"ready":true,"fixtureProof":proof_hex})
    );
    io::stdout().flush()?;
    line.clear();
    reader.read_line(&mut line)?;
    if line.trim_end() != "run" {
        return Err("run-command".into());
    }
    let mut successes = 0;
    let mut legacy_results = 0;
    let mut samples = Vec::with_capacity(input.iterations);
    for iteration in 0..input.warmups + input.iterations {
        if iteration == input.warmups {
            std::thread::sleep(std::time::Duration::from_secs(1));
            println!("{}", serde_json::json!({"warmed":true}));
            io::stdout().flush()?;
            line.clear();
            reader.read_line(&mut line)?;
            if line.trim_end() != "measure" {
                return Err("measure-command".into());
            }
            successes = 0;
            legacy_results = 0;
        }
        let started = Instant::now();
        for index in 0..input.batch_size {
            if input.operation.starts_with("auth-") {
                let hash = match input.operation.as_str() {
                    "auth-current" | "auth-rejected" => &current_hash,
                    "auth-legacy-pbkdf2" => &legacy_hash,
                    "auth-legacy-sha256" => &sha_hash,
                    _ => return Err("operation".into()),
                };
                let candidate = if input.operation == "auth-rejected" {
                    &rejected_password
                } else {
                    &input.password
                };
                let (accepted, old) = verify(candidate.as_bytes(), hash, accept_sha);
                if accepted != (input.operation != "auth-rejected")
                    || old != input.operation.starts_with("auth-legacy-")
                {
                    return Err("auth-parity".into());
                }
                successes += usize::from(accepted);
                legacy_results += usize::from(old);
            } else {
                let pair = index % input.residents;
                let first = &users[pair * 2];
                let second = &users[pair * 2 + 1];
                match input.operation.as_str() {
                    "registry-lookup" => {
                        let found = registry.get(first).ok_or("lookup")?;
                        let peer = if found.first.as_ref() == first.as_ref() {
                            &found.second
                        } else {
                            &found.first
                        };
                        if peer != second {
                            return Err("peer".into());
                        }
                        successes += 1;
                    }
                    "registry-churn" => {
                        let session = registry.start(first, second);
                        let suspended = registry.suspend(first).ok_or("suspend")?;
                        let resumed = registry.resume(first, &session.id).ok_or("resume")?;
                        let stale = registry.resume(first, "");
                        let ended = registry.end(first);
                        if suspended.expires_at <= 0
                            || resumed.expires_at != 0
                            || stale.is_some()
                            || ended.is_none()
                            || registry.get(second).is_some()
                        {
                            return Err("registry-parity".into());
                        }
                        successes += 1;
                    }
                    "registry-expire" => {
                        for seed in 0..input.residents {
                            registry.start(&users[seed * 2], &users[seed * 2 + 1]);
                            registry.suspend(&users[seed * 2]);
                        }
                        let expired = registry.expire(i64::MAX);
                        if expired.len() != input.residents {
                            return Err("expire-parity".into());
                        }
                        successes += expired.len();
                    }
                    _ => return Err("operation".into()),
                }
            }
        }
        if iteration >= input.warmups {
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    println!(
        "{}",
        serde_json::to_string(&ResultData {
            operation: input.operation,
            engine: input.engine,
            batch_ms: samples,
            successes,
            legacy_results
        })?
    );
    io::stdout().flush()?;
    line.clear();
    reader.read_line(&mut line)?;
    if line.trim_end() != "quit" {
        return Err("quit-command".into());
    }
    Ok(())
}

fn main() {
    if run().is_err() {
        eprintln!("benchmark-worker-failed");
        std::process::exit(1);
    }
}
