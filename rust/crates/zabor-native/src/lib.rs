#![forbid(unsafe_code)]

pub mod achievements;
pub mod attestation;
#[cfg(windows)]
pub mod audio_device;
#[cfg(feature = "native-media")]
pub mod audio_mix;
#[cfg(feature = "native-media")]
pub mod audio_playback;
#[cfg(feature = "native-media")]
pub mod audio_preferences;
pub mod audio_route;
#[cfg(feature = "native-media")]
pub mod audio_runtime;
pub mod audio_settings;
pub mod auth;
pub mod avatar;
pub mod call_recovery;
#[cfg(all(windows, feature = "native-media"))]
pub mod camera;
pub mod chat;
pub mod chat_crypto;
pub mod chat_store;
#[cfg(feature = "native-media")]
pub mod chat_transport;
pub mod critical_intents;
#[cfg(feature = "native-media")]
pub mod file_jobs;
pub mod files;
pub mod hub;
pub mod localization;
pub mod manual_gate;
#[cfg(feature = "native-media")]
pub mod media;
pub mod mic_test;
#[cfg(feature = "native-media")]
pub mod mic_test_ui;
#[cfg(feature = "native-media")]
pub mod microphone;
pub mod motion;
#[cfg(all(windows, feature = "native-media"))]
pub mod native_audio;
#[cfg(feature = "neural-dsp")]
pub mod neural;
#[cfg(feature = "native-media")]
pub mod picker;
pub mod presence;
pub mod profile;
pub mod protocol;
#[cfg(test)]
#[path = "../tests/reference/contract.rs"]
mod reference;
pub mod release;
#[cfg(all(windows, feature = "native-media"))]
mod remote_input;
#[cfg(feature = "native-media")]
pub mod remote_viewer;
pub mod roster;
#[cfg(feature = "native-media")]
pub mod screen;
pub mod smart_gate;
#[cfg(all(windows, feature = "native-media"))]
pub mod stream_audio;
pub mod strings;
pub mod update;
#[cfg(windows)]
pub mod update_helper;
#[cfg(windows)]
pub mod update_host;
#[cfg(windows)]
pub mod update_io;
#[cfg(feature = "native-media")]
pub mod video;
#[cfg(feature = "native-media")]
pub mod voice;
pub mod voice_activity;
pub mod voice_filters;
