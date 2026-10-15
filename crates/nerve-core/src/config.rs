//! Loading config files: a committed base file plus an optional local

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Broker connection settings, shared by every service.
///
/// Deliberately not `Debug`, so the password can't end up in a log.
#[derive(Clone, Deserialize)]
pub struct MqttConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Topic root: `home` on the Pi, `dev` on the dev machine.
    pub topic_root: String,
    pub username: String,
    pub password: String,
}

fn default_port() -> u16 {
    1883
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("can't read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("can't parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid config: {0}")]
    Invalid(toml::de::Error),
}

/// Loads `path`, then lays the matching `.local.toml` file over it if one
/// exists. Values in the local file win.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let mut table = read_table(path)?;

    let local_path = path.with_extension("local.toml");
    if local_path.exists() {
        merge(&mut table, read_table(&local_path)?);
    }

    T::deserialize(table).map_err(ConfigError::Invalid)
}

fn read_table(path: &Path) -> Result<toml::Table, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    text.parse().map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Copies every value in `overlay` into `base`. Tables present in both
/// are merged key by key rather than replaced.
fn merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base_table)), toml::Value::Table(overlay_table)) => {
                merge(base_table, overlay_table);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct TestConfig {
        mqtt: MqttConfig,
    }

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn local_values_are_merged_into_the_base() {
        let mut base = table(
            r#"
            [mqtt]
            host = "192.168.1.252"
            topic_root = "dev"
            "#,
        );
        let local = table(
            r#"
            [mqtt]
            username = "dylan"
            password = "secret"
            "#,
        );
        merge(&mut base, local);

        let config = TestConfig::deserialize(base).unwrap();
        assert_eq!(config.mqtt.host, "192.168.1.252");
        assert_eq!(config.mqtt.port, 1883);
        assert_eq!(config.mqtt.username, "dylan");
    }

    #[test]
    fn local_values_win() {
        let mut base = table("[mqtt]\nhost = \"base\"");
        merge(&mut base, table("[mqtt]\nhost = \"local\""));
        assert_eq!(base["mqtt"]["host"].as_str(), Some("local"));
    }

    #[test]
    fn a_missing_secret_is_an_error() {
        let base = table("[mqtt]\nhost = \"h\"\ntopic_root = \"dev\"\nusername = \"u\"");
        let error = TestConfig::deserialize(base).err().unwrap();
        assert!(error.to_string().contains("password"), "{error}");
    }
}
