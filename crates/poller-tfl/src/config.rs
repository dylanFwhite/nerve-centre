//! poller-tfl's settings: the shared `[mqtt]` section plus `[tfl]`.

use nerve_core::config::MqttConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub tfl: TflConfig,
}

#[derive(Deserialize)]
pub struct TflConfig {
    /// Optional: TfL allows a limited number of requests without a key.
    pub app_key: Option<String>,
    pub status_poll_seconds: u64,
    pub planned_poll_seconds: u64,
    pub planned_days: u32,
}
