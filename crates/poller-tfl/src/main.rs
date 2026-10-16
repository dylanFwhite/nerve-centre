mod config;
mod tfl;

use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use nerve_core::mqtt::Publisher;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config/dev.toml".into()),
    );
    let config: config::Config = nerve_core::config::load(&path)?;

    let publisher = Publisher::connect(&config.mqtt, "poller-tfl")?;

    // Temporary: publish the saved TfL response, to see it arrive.
    let status = tfl::parse_status(include_bytes!("../fixtures/line-status.json"))?;
    publisher
        .publish(&publisher.topics().tfl_status(), status)
        .await?;
    publisher.report_online(Some(Utc::now())).await?;
    println!("published; waiting 5 s for the messages to be sent");

    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}
