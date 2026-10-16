mod config;
mod tfl;

use ::std::pin::pin;
use core::error;
use std::path::PathBuf;
use std::time::Duration;
use std::{error::Error, net::Shutdown};

use chrono::{DateTime, Utc};
use nerve_core::mqtt::Publisher;
use nerve_core::payloads::tfl::TflStatus;
use nerve_core::shutdown::shutdown_signal;
use tokio::time::{MissedTickBehavior, interval};

const SERVICE: &str = "poller-tfl";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config/dev.toml".into()),
    );
    let config: config::Config = nerve_core::config::load(&path)?;

    let api = tfl::Api::new(config.tfl.app_key.clone())?;
    let publisher = Publisher::connect(&config.mqtt, "poller-tfl")?;

    let mut ticker = interval(Duration::from_secs(config.tfl.status_poll_seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut shutdown = pin!(shutdown_signal());
    let mut last_published: Option<TflStatus> = None;
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = &mut shutdown => break,
        }

        match api.fetch_status().await {
            Ok(status) => {
                last_success = Some(Utc::now());
                if last_published.as_ref() != Some(&status) {
                    publisher
                        .publish(&publisher.topics().tfl_status(), status.clone())
                        .await?;
                    println!("tfl: status changed, published");
                    last_published = Some(status);
                }
            }
            Err(error) => eprintln!("tfl: {error}"),
        }

        publisher.report_online(last_success).await?;
    }

    println!("shutting down");
    publisher.disconnect(last_success).await?;
    Ok(())
}
