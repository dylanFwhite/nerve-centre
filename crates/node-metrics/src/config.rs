//! node-metrics' settings: the shared `[mqtt]` section plus `[node]`.

use nerve_core::config::MqttConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub node: NodeConfig,
}

#[derive(Deserialize)]
pub struct NodeConfig {
    pub name: Option<String>,
    pub interval_seconds: u64,
    pub units: Vec<String>,
}
