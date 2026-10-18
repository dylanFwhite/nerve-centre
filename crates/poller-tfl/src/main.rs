mod config;
mod tfl;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use nerve_core::mqtt::Publisher;
use nerve_core::poll::{PublishOnChange, ticker};
use nerve_core::shutdown::shutdown_signal;

const SERVICE: &str = "poller-tfl";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
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
    let mut status_out = PublishOnChange::new(publisher.topics().tfl_status());
    let mut planned_out = PublishOnChange::new(publisher.topics().tfl_planned());

    let mut shutdown = pin!(shutdown_signal());
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            _ = status_ticker.tick() => {
                match api.fetch_status().await {
                    Ok(status) => {
                        last_success = Some(Utc::now());
                        status_out.publish(&publisher, status).await?
                    }
                    Err(error) => eprintln!("tfl status: {error}")
                }
                publisher.report_online(last_success).await?;
            }

            _ = planned_ticker.tick() => {
                match api.fetch_planned(config.tfl.planned_days).await {
                    Ok(planned) => planned_out.publish(&publisher, planned).await?,
                    Err(error) => eprintln!("tfl planned: {error}")
                }
            }
        }
    }

    println!("shutting down");
    publisher.disconnect(last_success).await?;
    Ok(())
}
