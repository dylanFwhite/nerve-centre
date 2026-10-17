mod config;
mod tfl;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;
use std::time::Duration;

use chrono::{DateTime, Utc};
use nerve_core::envelope::Payload;
use nerve_core::mqtt::{MqttError, Publisher};
use nerve_core::payloads::tfl::{TflPlanned, TflStatus};
use nerve_core::shutdown::shutdown_signal;
use tokio::time::{Interval, MissedTickBehavior, interval};

const SERVICE: &str = "poller-tfl";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config/dev.toml".into()),
    );
    let config: config::Config = nerve_core::config::load(&path)?;

    let api = tfl::Api::new(config.tfl.app_key.clone(), &config.tfl.lines)?;
    let publisher = Publisher::connect(&config.mqtt, SERVICE)?;

    let mut status_ticker = ticker(config.tfl.status_poll_seconds);
    let mut planned_ticker = ticker(config.tfl.planned_poll_seconds);

    let mut shutdown = pin!(shutdown_signal());
    let mut last_status: Option<TflStatus> = None;
    let mut last_planned: Option<TflPlanned> = None;
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            _ = status_ticker.tick() => {
                match api.fetch_status().await {
                    Ok(status) => {
                        last_success = Some(Utc::now());
                        let topic = publisher.topics().tfl_status();
                        published_if_changed(&publisher, &topic, status, &mut last_status).await?;
                    }
                    Err(error) => eprintln!("tfl status: {error}")
                }
                publisher.report_online(last_success).await?;
            }

            _ = planned_ticker.tick() => {
                match api.fetch_planned(config.tfl.planned_days).await {
                    Ok(planned) => {
                        let topic = publisher.topics().tfl_planned();
                        published_if_changed(&publisher, &topic, planned, &mut last_planned).await?;
                    }
                    Err(error) => eprintln!("tfl planned: {error}")
                }
            }
        }
    }

    println!("shutting down");
    publisher.disconnect(last_success).await?;
    Ok(())
}

fn ticker(seconds: u64) -> Interval {
    let mut ticker = interval(Duration::from_secs(seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

async fn published_if_changed<T: Payload + Clone>(
    publisher: &Publisher,
    topic: &str,
    data: T,
    last: &mut Option<T>,
) -> Result<(), MqttError> {
    if last.as_ref() != Some(&data) {
        publisher.publish(topic, data.clone()).await?;
        println!("published {topic}");
        *last = Some(data);
    }

    Ok(())
}
