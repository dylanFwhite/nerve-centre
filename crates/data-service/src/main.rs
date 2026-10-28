mod config;
mod recorder;
mod store;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use nerve_core::mqtt::Connection;
use nerve_core::poll::ticker;
use nerve_core::shutdown::shutdown_signal;
use nerve_core::topics::Topics;

use recorder::{Outcome, Recorder};
use store::Store;

const SERVICE: &str = "data-service";
const HEARTBEAT_SECONDS: u64 = 60;

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

    if let Some(folder) = config.store.path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let store = Store::open(&config.store.path)?;

    let topics = Topics::new(&config.mqtt.topic_root)?;
    let mut recorder = Recorder::new(store, topics.service(""))?;
    let (mqtt, mut messages) = Connection::subscribe(&config.mqtt, SERVICE, topics.all())?;

    let mut heartbeat = ticker(HEARTBEAT_SECONDS);
    let mut shutdown = pin!(shutdown_signal());
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            _ = heartbeat.tick() => mqtt.report_online(last_success).await?,

            message = messages.recv() => {
                let Some(message) = message else {
                    return Err("MQTT event loop stopped".into());
                };
                let topic = message.topic();

                match recorder.record(topic, message.payload(), Utc::now())? {
                    Outcome::Stored => println!("stored {topic}"),
                    Outcome::Invalid(reason) => eprintln!("skipped {topic}: {reason}"),
                    Outcome::Duplicate | Outcome::Unchanged => {}
                }
                mqtt.ack(&message).await?;
                last_success = Some(Utc::now());
            }
        }
    }

    println!("shutting down");
    mqtt.disconnect(last_success).await?;
    Ok(())
}
