//! Prints every message under the dev topics, for checking what services
//! publish. Run from the project root with:
//!
//!     cargo run -p nerve-core --example listen

use std::error::Error;
use std::path::Path;
use std::pin::pin;

use nerve_core::config::{self, MqttConfig};
use nerve_core::mqtt::Connection;
use nerve_core::shutdown::shutdown_signal;
use nerve_core::topics::Topics;
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    mqtt: MqttConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config: Config = config::load(Path::new("config/dev.toml"))?;
    let filter = Topics::new(&config.mqtt.topic_root)?.all();
    let (mqtt, mut messages) = Connection::subscribe(&config.mqtt, "listen", filter)?;

    let mut shutdown = pin!(shutdown_signal());

    loop {
        tokio::select! {
          _ = &mut shutdown => break,
          Some(message) = messages.recv() => {
            let payload = String::from_utf8_lossy(message.payload());
            println!("{}: {payload}", message.topic());
            mqtt.ack(&message).await?;
          }
        }
    }

    mqtt.disconnect(None).await?;
    Ok(())
}
