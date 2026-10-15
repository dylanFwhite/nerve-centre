mod config;
mod tfl;

use std::error::Error;
use std::path::PathBuf;

use nerve_core::topics::Topics;

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config/dev.toml".into()),
    );
    let config: config::Config = nerve_core::config::load(&path)?;
    let topics = Topics::new(&config.mqtt.topic_root)?;

    println!("broker:        {}:{}", config.mqtt.host, config.mqtt.port);
    println!("publishing to: {}", topics.tfl_status());
    println!(
        "TfL app key:   {}",
        if config.tfl.app_key.is_some() {
            "set"
        } else {
            "not set"
        }
    );
    Ok(())
}
