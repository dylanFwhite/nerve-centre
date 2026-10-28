//! data-service's settings: the shared `[mqtt]` section plus `[store]`.

use std::path::PathBuf;

use nerve_core::config::MqttConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub store: StoreConfig,
}

#[derive(Deserialize)]
pub struct StoreConfig {
    /// The path to the SQLite db file
    pub path: PathBuf,
}
