mod config;
mod node;
mod sampler;
mod services;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use nerve_core::mqtt::Connection;
use nerve_core::poll::ticker;
use nerve_core::shutdown::shutdown_signal;

use sampler::{Sample, Sampler};

const SERVICE: &str = "node-metrics";

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
    let settings = config.node;

    let name = match settings.name {
        Some(name) => name,
        None => node::hostname()?,
    };
    if name.is_empty() || name.contains(['/', '+', '#']) {
        return Err(format!("invalid node name {name:?}").into());
    }

    let mqtt = Connection::connect(&config.mqtt, &format!("{SERVICE}@{name}"))?;
    let topic = mqtt.topics().node_metrics(&name);

    let cores = std::thread::available_parallelism()?.get();
    let mut sampler = Sampler::new(settings.interval_seconds, cores);
    let mut ticker = ticker(settings.interval_seconds);
    let mut shutdown = pin!(shutdown_signal());
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            _ = ticker.tick() => {
                match Sample::take(&settings.units) {
                    Ok(sample) => {
                        last_success = Some(Utc::now());
                        if let Some(metrics) = sampler.next(sample) {
                            mqtt.publish(&topic, metrics).await?;
                        }
                    },
                    Err(error) => eprintln!("sample: {error}")
                }
                mqtt.report_online(last_success).await?;
            }
        }
    }

    println!("shutting down");
    mqtt.disconnect(last_success).await?;
    Ok(())
}
