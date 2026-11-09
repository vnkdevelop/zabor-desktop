use crate::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use crate::hub::{HubConnection, HubError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_IDS: usize = 2048;
pub const MAX_HIDDEN: usize = 256;
pub const STORAGE_KEY: &str = "zabor_unlocked_hidden_achievements_v1";
const MAX_NUMBER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AchievementError {
    #[error("achievement request cancelled")]
    Cancelled,
    #[error("achievement response missing")]
    Missing,
    #[error("invalid achievement response")]
    Invalid,
    #[error("achievement response exceeds limits")]
    Limit,
    #[error("achievement request already pending")]
    Busy,
    #[error("achievement request failed")]
    Hub(HubError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Voice,
    Calls,
    Social,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Unit {
    #[serde(rename = "min")]
    Minutes,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenDefinition {
    #[serde(alias = "Id")]
    pub id: String,
    #[serde(alias = "Icon")]
    pub icon: String,
    #[serde(alias = "MaxValue")]
    pub max_value: f64,
    #[serde(alias = "StatKey")]
    pub stat_key: String,
    #[serde(default, alias = "Unit")]
    pub unit: Option<Unit>,
    #[serde(default, alias = "Category")]
    pub category: Option<Category>,
    #[serde(alias = "TitleRu")]
    pub title_ru: String,
    #[serde(alias = "DescriptionRu")]
    pub description_ru: String,
    #[serde(alias = "TitleEn")]
    pub title_en: String,
    #[serde(alias = "DescriptionEn")]
    pub description_en: String,
}

impl HiddenDefinition {
    fn validate(&self) -> Result<(), AchievementError> {
        valid_id(&self.id)?;
        valid_id(&self.stat_key)?;
        if STANDARD.iter().any(|definition| definition.id == self.id)
            || !self.max_value.is_finite()
            || self.max_value <= 0.0
            || self.max_value > MAX_NUMBER
        {
            return Err(AchievementError::Invalid);
        }
        for (text, maximum) in [
            (&self.icon, 64),
            (&self.title_ru, 4096),
            (&self.description_ru, 4096),
            (&self.title_en, 4096),
            (&self.description_en, 4096),
        ] {
            if text.len() > maximum {
                return Err(AchievementError::Limit);
            }
            if text.contains('\0') {
                return Err(AchievementError::Invalid);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AchievementData {
    pub stats: BTreeMap<String, f64>,
    pub unlocked_ids: Vec<String>,
    pub visited_channel_ids: Vec<String>,
    pub total_hidden_count: usize,
    pub unlocked_hidden_defs: Vec<HiddenDefinition>,
}

impl Default for AchievementData {
    fn default() -> Self {
        Self {
            stats: BTreeMap::new(),
            unlocked_ids: Vec::new(),
            visited_channel_ids: Vec::new(),
            total_hidden_count: 10,
            unlocked_hidden_defs: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    #[serde(default, alias = "Stats")]
    stats: Option<UniqueMap<f64>>,
    #[serde(default, alias = "UnlockedIds")]
    unlocked_ids: Option<Vec<String>>,
    #[serde(default, alias = "VisitedChannelIds")]
    visited_channel_ids: Option<Vec<String>>,
    #[serde(default, alias = "TotalHiddenCount")]
    total_hidden_count: Option<usize>,
    #[serde(default, alias = "UnlockedHiddenDefs")]
    unlocked_hidden_defs: Option<Vec<HiddenDefinition>>,
}

struct UniqueMap<T>(BTreeMap<String, T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for UniqueMap<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
            type Value = UniqueMap<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map with unique keys")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<String, T>()? {
                    if map.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                }
                Ok(UniqueMap(map))
            }
        }
        deserializer.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

fn valid_id(id: &str) -> Result<(), AchievementError> {
    if id.len() > 256 {
        return Err(AchievementError::Limit);
    }
    if id.trim().is_empty() || id.chars().any(char::is_control) {
        return Err(AchievementError::Invalid);
    }
    Ok(())
}

fn unique_ids(ids: Vec<String>) -> Result<Vec<String>, AchievementError> {
    if ids.len() > MAX_IDS {
        return Err(AchievementError::Limit);
    }
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for id in ids {
        valid_id(&id)?;
        if seen.insert(id.clone()) {
            unique.push(id);
        }
    }
    Ok(unique)
}

pub fn parse_result(result: &Value) -> Result<AchievementData, AchievementError> {
    if result.is_null() || result.as_str() == Some("") {
        return Err(AchievementError::Missing);
    }
    let source = result.as_str().ok_or(AchievementError::Invalid)?;
    if source.len() > MAX_PAYLOAD_BYTES {
        return Err(AchievementError::Limit);
    }
    let shape: Value = serde_json::from_str(source).map_err(|_| AchievementError::Invalid)?;
    if !shape.is_object() {
        return Err(AchievementError::Invalid);
    }
    let envelope: Envelope = serde_json::from_str(source).map_err(|_| AchievementError::Invalid)?;
    let stats = envelope.stats.map(|map| map.0).unwrap_or_default();
    if stats.len() > 128 {
        return Err(AchievementError::Limit);
    }
    for (key, number) in &stats {
        valid_id(key)?;
        if !number.is_finite() || *number < 0.0 || *number > MAX_NUMBER {
            return Err(AchievementError::Invalid);
        }
    }
    let total_hidden_count = envelope.total_hidden_count.unwrap_or(10);
    let definitions = envelope.unlocked_hidden_defs.unwrap_or_default();
    if total_hidden_count > MAX_HIDDEN || definitions.len() > MAX_HIDDEN {
        return Err(AchievementError::Limit);
    }
    let mut unique = BTreeMap::<String, HiddenDefinition>::new();
    let mut unlocked_hidden_defs = Vec::new();
    for definition in definitions {
        definition.validate()?;
        if let Some(previous) = unique.get(&definition.id) {
            if previous != &definition {
                return Err(AchievementError::Invalid);
            }
        } else {
            unique.insert(definition.id.clone(), definition.clone());
            unlocked_hidden_defs.push(definition);
        }
    }
    Ok(AchievementData {
        stats,
        unlocked_ids: unique_ids(envelope.unlocked_ids.unwrap_or_default())?,
        visited_channel_ids: unique_ids(envelope.visited_channel_ids.unwrap_or_default())?,
        total_hidden_count,
        unlocked_hidden_defs,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AchievementTarget {
    Own,
    Peer(String),
}

#[derive(Clone, Debug)]
pub struct AchievementQuery {
    owner: String,
    target: AchievementTarget,
    generation: u64,
    ticket: IntentTicket,
    stop: Arc<CancellationToken>,
}

impl PartialEq for AchievementQuery {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.target == other.target
            && self.generation == other.generation
            && self.ticket == other.ticket
            && Arc::ptr_eq(&self.stop, &other.stop)
    }
}

impl Eq for AchievementQuery {}

impl AchievementQuery {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn target(&self) -> &AchievementTarget {
        &self.target
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn ticket(&self) -> IntentTicket {
        self.ticket
    }

    pub fn alive(&self) -> bool {
        !self.stop.is_cancelled()
    }

    pub async fn cancelled(&self) {
        self.stop.cancelled().await;
    }
}

pub async fn load(
    hub: &HubConnection,
    intents: &CriticalIntents,
    shutdown: &CancellationToken,
    query: &AchievementQuery,
) -> Result<AchievementData, AchievementError> {
    load_timed(hub, intents, shutdown, query, Duration::from_secs(20)).await
}

async fn load_timed(
    hub: &HubConnection,
    intents: &CriticalIntents,
    shutdown: &CancellationToken,
    query: &AchievementQuery,
    timeout: Duration,
) -> Result<AchievementData, AchievementError> {
    let (method, arguments) = match &query.target {
        AchievementTarget::Own => ("GetMyAchievements", vec![]),
        AchievementTarget::Peer(id) => ("GetUserAchievements", vec![Value::String(id.clone())]),
    };
    let response = tokio::select! {
        biased;
        _ = shutdown.cancelled() => return Err(AchievementError::Cancelled),
        _ = query.stop.cancelled() => return Err(AchievementError::Cancelled),
        response = intents.run(query.ticket, IntentScope::Session, hub.invoke(method, arguments, timeout)) => response,
    };
    if shutdown.is_cancelled()
        || !query.alive()
        || !intents.is_current(query.ticket, IntentScope::Session)
    {
        return Err(AchievementError::Cancelled);
    }
    let result = response
        .ok_or(AchievementError::Cancelled)?
        .map_err(AchievementError::Hub)?;
    let parsed = parse_result(&result);
    if shutdown.is_cancelled()
        || !query.alive()
        || !intents.is_current(query.ticket, IntentScope::Session)
    {
        return Err(AchievementError::Cancelled);
    }
    parsed
}

#[derive(Default)]
pub struct AchievementsView {
    generation: u64,
    query: Option<AchievementQuery>,
    data: Option<AchievementData>,
    error: Option<AchievementError>,
    pending: bool,
}

impl AchievementsView {
    pub fn begin(
        &mut self,
        intents: &CriticalIntents,
        ticket: IntentTicket,
        owner: &str,
        target: AchievementTarget,
    ) -> Result<AchievementQuery, AchievementError> {
        valid_id(owner)?;
        if let AchievementTarget::Peer(id) = &target {
            valid_id(id)?;
            if id == owner {
                return Err(AchievementError::Invalid);
            }
        }
        if !intents.is_current(ticket, IntentScope::Session) {
            return Err(AchievementError::Cancelled);
        }
        if self.pending
            && self.query.as_ref().is_some_and(|query| {
                query.owner == owner
                    && query.target == target
                    && intents.is_current(query.ticket, IntentScope::Session)
            })
        {
            return Err(AchievementError::Busy);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(AchievementError::Limit)?;
        if let Some(query) = &self.query {
            query.stop.cancel();
        }
        let query = AchievementQuery {
            owner: owner.to_owned(),
            target,
            generation: self.generation,
            ticket,
            stop: Arc::new(CancellationToken::new()),
        };
        self.query = Some(query.clone());
        self.data = None;
        self.error = None;
        self.pending = true;
        Ok(query)
    }

    pub fn close(&mut self) {
        if let Some(query) = &self.query {
            query.stop.cancel();
        }
        self.query = None;
        self.data = None;
        self.error = None;
        self.pending = false;
    }

    pub fn query(&self) -> Option<&AchievementQuery> {
        self.query.as_ref()
    }

    pub fn data(&self) -> Option<&AchievementData> {
        self.data.as_ref()
    }

    pub fn error(&self) -> Option<&AchievementError> {
        self.error.as_ref()
    }

    pub fn accept(
        &mut self,
        intents: &CriticalIntents,
        query: &AchievementQuery,
        result: Result<AchievementData, AchievementError>,
    ) -> bool {
        if self.query.as_ref() != Some(query) || !query.alive() || !self.pending {
            return false;
        }
        intents
            .commit(query.ticket, IntentScope::Session, || {
                self.pending = false;
                match result {
                    Ok(data) => {
                        self.data = Some(data);
                        self.error = None;
                    }
                    Err(error) => {
                        self.data = Some(AchievementData::default());
                        self.error = Some(error);
                    }
                }
            })
            .is_some()
    }

    pub fn rows(
        &self,
        language: &str,
        translate: impl Fn(&str) -> String,
        own_catalog: Option<&HiddenCatalog>,
    ) -> Vec<AchievementRow> {
        let (Some(query), Some(data)) = (&self.query, &self.data) else {
            return Vec::new();
        };
        presentation(data, query, language, translate, own_catalog)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HiddenCatalog {
    owner: String,
    definitions: BTreeMap<String, HiddenDefinition>,
}

impl HiddenCatalog {
    pub fn empty(owner: &str) -> Result<Self, AchievementError> {
        valid_id(owner)?;
        Ok(Self {
            owner: owner.to_owned(),
            definitions: BTreeMap::new(),
        })
    }

    pub fn from_durable(owner: &str, source: &str) -> Result<Self, AchievementError> {
        let mut catalog = Self::empty(owner)?;
        if source.len() > MAX_PAYLOAD_BYTES {
            return Err(AchievementError::Limit);
        }
        let shape: Value = serde_json::from_str(source).map_err(|_| AchievementError::Invalid)?;
        if !shape.is_object() {
            return Err(AchievementError::Invalid);
        }
        let definitions: UniqueMap<HiddenDefinition> =
            serde_json::from_str(source).map_err(|_| AchievementError::Invalid)?;
        if definitions.0.len() > MAX_HIDDEN {
            return Err(AchievementError::Limit);
        }
        for (id, definition) in &definitions.0 {
            definition.validate()?;
            if id != &definition.id {
                return Err(AchievementError::Invalid);
            }
        }
        catalog.definitions = definitions.0;
        Ok(catalog)
    }

    pub fn prepare(
        &self,
        view: &AchievementsView,
    ) -> Result<Option<HiddenPersistence>, AchievementError> {
        let (Some(query), Some(data)) = (&view.query, &view.data) else {
            return Ok(None);
        };
        if query.target != AchievementTarget::Own
            || query.owner != self.owner
            || !query.alive()
            || view.error.is_some()
            || view.pending
        {
            return Ok(None);
        }
        let mut catalog = self.clone();
        for definition in &data.unlocked_hidden_defs {
            definition.validate()?;
            catalog
                .definitions
                .insert(definition.id.clone(), definition.clone());
        }
        if catalog.definitions.len() > MAX_HIDDEN {
            return Err(AchievementError::Limit);
        }
        let json =
            serde_json::to_string(&catalog.definitions).map_err(|_| AchievementError::Invalid)?;
        if json.len() > MAX_PAYLOAD_BYTES {
            return Err(AchievementError::Limit);
        }
        Ok(Some(HiddenPersistence {
            query: query.clone(),
            catalog,
            json,
        }))
    }

    pub fn prepare_response(
        &self,
        query: &AchievementQuery,
        data: &AchievementData,
    ) -> Result<Option<HiddenPersistence>, AchievementError> {
        self.prepare(&AchievementsView {
            query: Some(query.clone()),
            data: Some(data.clone()),
            pending: false,
            ..Default::default()
        })
    }
}

pub struct HiddenPersistence {
    query: AchievementQuery,
    catalog: HiddenCatalog,
    json: String,
}

impl HiddenPersistence {
    pub fn owner(&self) -> &str {
        &self.query.owner
    }

    pub fn prepared_json(&self) -> &str {
        &self.json
    }

    pub fn commit_if_current(
        self,
        intents: &CriticalIntents,
        view: &AchievementsView,
    ) -> Option<HiddenCatalog> {
        if view.query.as_ref() != Some(&self.query) || view.pending || view.error.is_some() {
            return None;
        }
        intents
            .commit(self.query.ticket, IntentScope::Session, || {
                self.query.alive()
            })
            .filter(|alive| *alive)
            .map(|_| self.catalog)
    }
}

pub struct StandardDefinition {
    pub id: &'static str,
    pub icon: &'static str,
    pub max_value: f64,
    pub stat_key: &'static str,
    pub category: Category,
    pub unit: Option<Unit>,
}

macro_rules! definition {
    ($id:literal, $icon:literal, $max:literal, $stat:literal, $category:ident) => {
        StandardDefinition {
            id: $id,
            icon: $icon,
            max_value: $max,
            stat_key: $stat,
            category: Category::$category,
            unit: None,
        }
    };
    ($id:literal, $icon:literal, $max:literal, $stat:literal, $category:ident, min) => {
        StandardDefinition {
            id: $id,
            icon: $icon,
            max_value: $max,
            stat_key: $stat,
            category: Category::$category,
            unit: Some(Unit::Minutes),
        }
    };
}

pub const STANDARD: [StandardDefinition; 15] = [
    definition!("first_channel", "🔭", 1.0, "channelsCreated", Voice),
    definition!("soul_1", "🗣️", 600.0, "totalVoiceMinutes", Voice, min),
    definition!("soul_2", "😜", 3000.0, "totalVoiceMinutes", Voice, min),
    definition!("soul_3", "🎙️", 6000.0, "totalVoiceMinutes", Voice, min),
    definition!("crowd", "👥", 8.0, "maxUsersInChannel", Voice),
    definition!("collector", "📚", 10.0, "uniqueChannels", Voice),
    definition!("modnik", "🪩", 1.0, "gifAvatarUploaded", Voice),
    definition!("victim", "🥲", 1.0, "timesKicked", Voice),
    definition!("first_call", "📞", 1.0, "totalCalls", Calls),
    definition!("same_wave", "🫂", 300.0, "longestCallMinutes", Calls, min),
    definition!("gossip", "🗿", 50.0, "totalCalls", Calls),
    definition!("busy", "🚫", 5.0, "declinedCalls", Calls),
    definition!("first_friend", "❤️", 1.0, "friendsCount", Social),
    definition!("magnet", "🧲", 20.0, "friendsCount", Social),
    definition!("popular", "⭐", 100.0, "profileViews", Social),
];

#[derive(Clone, Debug, PartialEq)]
pub struct AchievementRow {
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub category: Category,
    pub hidden: bool,
    pub unknown: bool,
    pub unlocked: bool,
    pub progress: f64,
    pub progress_text: String,
}

pub fn progress(value: f64, max: f64, unit: Option<Unit>, hours_unit: &str) -> (f64, String) {
    let safe = value.min(max);
    let (value, max) = if unit == Some(Unit::Minutes) {
        ((safe / 60.0).floor(), (max / 60.0).floor())
    } else {
        (safe, max)
    };
    let ratio = if max == 0.0 {
        0.0
    } else {
        (value / max).min(1.0)
    };
    let suffix = if unit == Some(Unit::Minutes) {
        hours_unit
    } else {
        ""
    };
    (ratio, format!("{value} / {max}{suffix}"))
}

fn presentation(
    data: &AchievementData,
    query: &AchievementQuery,
    language: &str,
    translate: impl Fn(&str) -> String,
    own_catalog: Option<&HiddenCatalog>,
) -> Vec<AchievementRow> {
    let unlocked: BTreeSet<&str> = data.unlocked_ids.iter().map(String::as_str).collect();
    let hours_unit = translate("achievements.hoursUnit");
    let mut ranked = Vec::<(AchievementRow, f64)>::new();
    for definition in &STANDARD {
        let stat = data.stats.get(definition.stat_key).copied().unwrap_or(0.0);
        let is_unlocked = unlocked.contains(definition.id);
        let effective = if is_unlocked {
            stat.max(definition.max_value)
        } else {
            stat
        };
        let (progress, progress_text) = progress(
            effective,
            definition.max_value,
            definition.unit,
            &hours_unit,
        );
        ranked.push((
            AchievementRow {
                id: definition.id.into(),
                title: translate(&format!("achievements.{}.title", definition.id)),
                description: translate(&format!("achievements.{}.description", definition.id)),
                icon: definition.icon.into(),
                category: definition.category,
                hidden: false,
                unknown: false,
                unlocked: is_unlocked,
                progress,
                progress_text,
            },
            (stat / definition.max_value).min(1.0),
        ));
    }
    let current: BTreeMap<&str, &HiddenDefinition> = data
        .unlocked_hidden_defs
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect();
    let own_catalog = own_catalog
        .filter(|catalog| query.target == AchievementTarget::Own && catalog.owner == query.owner);
    let mut hidden_count = 0;
    for id in &data.unlocked_ids {
        let definition = current
            .get(id.as_str())
            .copied()
            .or_else(|| own_catalog.and_then(|catalog| catalog.definitions.get(id)));
        let Some(definition) = definition else {
            continue;
        };
        hidden_count += 1;
        let stat = data.stats.get(&definition.stat_key).copied().unwrap_or(0.0);
        let (progress, progress_text) = progress(
            stat.max(definition.max_value),
            definition.max_value,
            definition.unit,
            &hours_unit,
        );
        ranked.push((
            AchievementRow {
                id: id.clone(),
                title: if language == "en" {
                    &definition.title_en
                } else {
                    &definition.title_ru
                }
                .clone(),
                description: if language == "en" {
                    &definition.description_en
                } else {
                    &definition.description_ru
                }
                .clone(),
                icon: definition.icon.clone(),
                category: Category::Hidden,
                hidden: true,
                unknown: false,
                unlocked: true,
                progress,
                progress_text,
            },
            (stat / definition.max_value).min(1.0),
        ));
    }
    ranked.sort_by(|(a, a_progress), (b, b_progress)| {
        let band = |row: &AchievementRow| if row.hidden { 0 } else { 1 };
        band(a)
            .cmp(&band(b))
            .then_with(|| b.unlocked.cmp(&a.unlocked))
            .then_with(|| b_progress.total_cmp(a_progress))
            .then_with(|| (a.category as u8).cmp(&(b.category as u8)))
    });
    let mut rows: Vec<_> = ranked.into_iter().map(|(row, _)| row).collect();
    if query.target == AchievementTarget::Own {
        for index in 0..data.total_hidden_count.saturating_sub(hidden_count) {
            rows.push(AchievementRow {
                id: format!("locked_hidden_{index}"),
                title: translate("achievements.hiddenTitle"),
                description: translate("achievements.hiddenDesc"),
                icon: "❓".into(),
                category: Category::Hidden,
                hidden: true,
                unknown: true,
                unlocked: false,
                progress: 0.0,
                progress_text: String::new(),
            });
        }
    }
    rows
}

#[cfg(test)]
#[path = "achievements/tests.rs"]
mod tests;
