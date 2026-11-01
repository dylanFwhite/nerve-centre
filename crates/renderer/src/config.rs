//! renderer's settings: the shared `[mqtt]` section. `[renderer]` arrives
//! with the code that uses it.

use nerve_core::config::MqttConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
}
