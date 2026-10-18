//! poller-weather's settings: the shared `[mqtt]` section plus `[weather]`.

use chrono_tz::Tz;
use nerve_core::config::MqttConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub weather: WeatherConfig,
}

#[derive(Deserialize)]
pub struct WeatherConfig {
    /// Name shown on the dashboard, e.g. `London`.
    pub location: String,
    pub latitude: f64,
    pub longitude: f64,
    /// IANA time zone name, e.g. `Europe/London`.
    pub timezone: Tz,
    pub poll_seconds: u64,
}
