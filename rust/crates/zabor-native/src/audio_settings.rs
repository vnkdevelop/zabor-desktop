use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseModel {
    #[default]
    Rnnoise,
    Deepfilter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThresholdMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    pub input_volume: f32,
    pub output_volume: f32,
    pub selected_input: String,
    pub selected_output: String,
    pub noise_suppression: bool,
    pub user_volumes: BTreeMap<String, f32>,
    pub language: String,
    pub mic_threshold_mode: ThresholdMode,
    pub manual_threshold_value: f32,
    pub smart_noise_model: NoiseModel,
    pub speech_analyzer: bool,
    pub echo_cancellation: bool,
    pub suppression_strength: f32,
    pub ultra_low_latency: bool,
    pub relay_only: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input_volume: 100.0,
            output_volume: 100.0,
            selected_input: "default".into(),
            selected_output: "default".into(),
            noise_suppression: true,
            user_volumes: BTreeMap::new(),
            language: "ru".into(),
            mic_threshold_mode: ThresholdMode::Auto,
            manual_threshold_value: -42.0,
            smart_noise_model: NoiseModel::Rnnoise,
            speech_analyzer: true,
            echo_cancellation: false,
            suppression_strength: 15.0,
            ultra_low_latency: false,
            relay_only: false,
            extra: Map::new(),
        }
    }
}

impl AudioSettings {
    pub fn restore(local: &Value, server: Option<&Value>) -> Self {
        let mut merged = local.as_object().cloned().unwrap_or_default();
        if let Some(server) = server.and_then(Value::as_object) {
            for (key, value) in server {
                if key != "language" {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        Self::from_json(&Value::Object(merged))
    }
    pub fn from_json(value: &Value) -> Self {
        let mut settings = Self::default();
        let Some(raw) = value.as_object() else {
            return settings;
        };
        settings.extra = raw.clone();
        let number = |key: &str, default: f32, min: f32, max: f32| {
            value[key]
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(f64::from(min), f64::from(max)) as f32)
                .unwrap_or(default)
        };
        settings.input_volume = number("inputVolume", 100.0, 0.0, 200.0);
        settings.output_volume = number("outputVolume", 100.0, 0.0, 200.0);
        settings.suppression_strength = number("suppressionStrength", 15.0, 5.0, 30.0).round();
        settings.manual_threshold_value = if value["manualThresholdValue"]
            .as_f64()
            .is_some_and(|value| value >= 0.0)
        {
            -42.0
        } else {
            number("manualThresholdValue", -42.0, -60.0, -12.0)
        };
        for (key, target) in [
            ("selectedInput", &mut settings.selected_input),
            ("selectedOutput", &mut settings.selected_output),
        ] {
            if let Some(value) = value[key]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 4096)
            {
                *target = value.into();
            }
        }
        for (key, target) in [
            ("noiseSuppression", &mut settings.noise_suppression),
            ("speechAnalyzer", &mut settings.speech_analyzer),
            ("echoCancellation", &mut settings.echo_cancellation),
            ("ultraLowLatency", &mut settings.ultra_low_latency),
            ("relayOnly", &mut settings.relay_only),
        ] {
            if let Some(value) = value[key].as_bool() {
                *target = value;
            }
        }
        if value["language"] == "en" {
            settings.language = "en".into();
        }
        if value["micThresholdMode"] == "manual" {
            settings.mic_threshold_mode = ThresholdMode::Manual;
        }
        if value["smartNoiseModel"] == "deepfilter" {
            settings.smart_noise_model = NoiseModel::Deepfilter;
        }
        if let Some(volumes) = value["userVolumes"].as_object() {
            settings.user_volumes = volumes
                .iter()
                .take(4096)
                .filter_map(|(id, volume)| {
                    volume
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .map(|value| (id.clone(), value.clamp(0.0, 200.0) as f32))
                })
                .collect();
        }
        for key in [
            "inputVolume",
            "outputVolume",
            "selectedInput",
            "selectedOutput",
            "noiseSuppression",
            "userVolumes",
            "language",
            "micThresholdMode",
            "manualThresholdValue",
            "smartNoiseModel",
            "speechAnalyzer",
            "echoCancellation",
            "suppressionStrength",
            "ultraLowLatency",
            "relayOnly",
        ] {
            settings.extra.remove(key);
        }
        settings
    }

    pub fn input_gain(&self) -> f32 {
        self.input_volume / 100.0
            * if self.noise_suppression && !self.ultra_low_latency {
                1.0
            } else {
                1.7
            }
    }
    pub fn peer_gain(&self, peer: &str) -> f32 {
        (self.output_volume * self.user_volumes.get(peer).copied().unwrap_or(100.0) / 10000.0)
            .clamp(0.0, 4.0)
    }
    pub fn post_filter_beta(&self) -> f32 {
        0.02 + 0.03
            * ((self.suppression_strength - 5.0) / 25.0)
                .clamp(0.0, 1.0)
                .powi(2)
    }

    pub fn edited(&self, key: &str, value: Value) -> Option<Self> {
        let valid = match key {
            "inputVolume" | "outputVolume" | "manualThresholdValue" | "suppressionStrength" => {
                value.as_f64().is_some_and(f64::is_finite)
            }
            "selectedInput" | "selectedOutput" => value
                .as_str()
                .is_some_and(|id| !id.is_empty() && id.len() <= 4096),
            "noiseSuppression" | "speechAnalyzer" | "echoCancellation" | "ultraLowLatency"
            | "relayOnly" => value.is_boolean(),
            "language" => value == "ru" || value == "en",
            "micThresholdMode" => value == "auto" || value == "manual",
            "smartNoiseModel" => value == "rnnoise" || value == "deepfilter",
            _ => false,
        };
        if !valid {
            return None;
        }
        let mut raw = serde_json::to_value(self).ok()?;
        raw[key] = value;
        Some(Self::from_json(&raw))
    }

    pub fn with_peer_volume(&self, peer: &str, volume: f32) -> Option<Self> {
        if peer.is_empty()
            || peer.len() > 4096
            || !volume.is_finite()
            || (!self.user_volumes.contains_key(peer) && self.user_volumes.len() >= 4096)
        {
            return None;
        }
        let mut settings = self.clone();
        settings
            .user_volumes
            .insert(peer.into(), volume.clamp(0.0, 200.0));
        Some(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_keep_unknown_fields_and_normalize_individual_legacy_values() {
        let settings = AudioSettings::from_json(
            &json!({"inputVolume":300,"outputVolume":-5,"manualThresholdValue":0.5,"selectedInput":null,"smartNoiseModel":"other","speechAnalyzer":false,"userVolumes":{"alice":175,"bob":500},"futureSetting":{"enabled":true}}),
        );
        assert_eq!(settings.input_volume, 200.0);
        assert_eq!(settings.output_volume, 0.0);
        assert_eq!(settings.manual_threshold_value, -42.0);
        assert_eq!(settings.selected_input, "default");
        assert_eq!(settings.smart_noise_model, NoiseModel::Rnnoise);
        assert!(!settings.speech_analyzer);
        assert_eq!(settings.user_volumes["bob"], 200.0);
        let wire = serde_json::to_value(&settings).unwrap();
        assert_eq!(wire["futureSetting"]["enabled"], true);
        assert_eq!(AudioSettings::from_json(&wire), settings);
    }

    #[test]
    fn gain_and_strength_curves_match_the_existing_client() {
        let settings = AudioSettings::from_json(
            &json!({"inputVolume":150,"outputVolume":200,"noiseSuppression":false,"suppressionStrength":30,"userVolumes":{"alice":200}}),
        );
        assert!((settings.input_gain() - 2.55).abs() < 1e-6);
        assert_eq!(settings.peer_gain("alice"), 4.0);
        assert!((settings.post_filter_beta() - 0.05).abs() < 1e-6);
    }

    #[test]
    fn server_audio_restore_preserves_local_language_and_unknown_fields() {
        let settings = AudioSettings::restore(
            &json!({"language":"en","inputVolume":25,"futureLocal":true}),
            Some(&json!({"language":"ru","inputVolume":175,"futureServer":{"active":true}})),
        );
        assert_eq!(settings.language, "en");
        assert_eq!(settings.input_volume, 175.0);
        assert_eq!(settings.extra["futureLocal"], true);
        assert_eq!(settings.extra["futureServer"]["active"], true);
    }

    #[test]
    fn typed_edits_reject_invalid_keys_and_preserve_unrelated_preferences() {
        let settings = AudioSettings::from_json(
            &json!({"language":"en","future":{"nested":true},"userVolumes":{"alice":160}}),
        );
        let edited = settings.edited("inputVolume", json!(250)).unwrap();
        assert_eq!(edited.input_volume, 200.0);
        assert_eq!(edited.output_volume, settings.output_volume);
        assert_eq!(edited.language, "en");
        assert_eq!(edited.extra, settings.extra);
        assert_eq!(edited.user_volumes, settings.user_volumes);
        for (key, value) in [
            ("unknown", json!(true)),
            ("language", json!("fr")),
            ("selectedInput", json!("")),
            ("noiseSuppression", json!(1)),
            ("inputVolume", Value::Null),
            ("smartNoiseModel", json!("other")),
        ] {
            assert!(settings.edited(key, value).is_none());
        }
        assert_eq!(
            settings
                .edited("suppressionStrength", json!(16.6))
                .unwrap()
                .suppression_strength,
            17.0
        );
        assert_eq!(
            settings
                .edited("manualThresholdValue", json!(-90))
                .unwrap()
                .manual_threshold_value,
            -60.0
        );
    }

    #[test]
    fn peer_volume_edits_are_bounded_and_do_not_change_other_users_or_global_gain() {
        let settings = AudioSettings::from_json(
            &json!({"outputVolume":175,"userVolumes":{"alice":150},"future":true}),
        );
        let edited = settings.with_peer_volume("bob", 300.0).unwrap();
        assert_eq!(edited.user_volumes["bob"], 200.0);
        assert_eq!(edited.user_volumes["alice"], 150.0);
        assert_eq!(edited.output_volume, 175.0);
        assert_eq!(edited.extra, settings.extra);
        assert!(settings.with_peer_volume("", 100.0).is_none());
        assert!(settings.with_peer_volume("bob", f32::NAN).is_none());
        let mut full = settings;
        full.user_volumes = (0..4096).map(|id| (id.to_string(), 100.0)).collect();
        assert!(full.with_peer_volume("new", 100.0).is_none());
        assert_eq!(
            full.with_peer_volume("1", -5.0).unwrap().user_volumes["1"],
            0.0
        );
    }
}
