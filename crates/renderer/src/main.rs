mod config;
mod state;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;
use std::process::ExitCode;

use nerve_core::mqtt::Connection;
use nerve_core::payloads::service::ServiceState;
use nerve_core::shutdown::shutdown_signal;
use nerve_core::topics::Topics;

use state::{Outcome, Received, State};

const SERVICE: &str = "renderer";

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

    let topics = Topics::new(&config.mqtt.topic_root)?;
    let mut state = State::new(&topics);
    let (mqtt, mut messages) =
        Connection::subscribe(&config.mqtt, SERVICE, State::filters(&topics))?;

    let mut shutdown = pin!(shutdown_signal());

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            message = messages.recv() => {
                let Some(message) = message else {
                    return Err("MQTT event loop stopped".into());
                };
                let topic = message.topic();

                match state.apply(topic, message.payload()) {
                    Ok(Outcome::Updated) => println!("{topic}: {}", summary(&state)),
                    Ok(Outcome::Older) => println!("{topic}: older than the data held"),
                    Ok(Outcome::Ignored) => {}
                    Err(error) => eprintln!("skipped {topic}: {error}"),
                }
                mqtt.ack(&message).await?;
            }
        }
    }

    println!("shutting down");
    mqtt.disconnect(None).await?;
    Ok(())
}

fn summary(state: &State) -> String {
    fn describe<T>(slot: &Option<Received<T>>, count: impl Fn(&T) -> String) -> String {
        match slot {
            Some(received) => format!(
                "{} at {}",
                count(&received.data),
                received.ts.format("%H:%M:%S")
            ),
            None => "none".to_string(),
        }
    }

    let online = state
        .services
        .values()
        .filter(|status| status.state == ServiceState::Online)
        .count();

    format!(
        "status {} | planned {} | forecast {} | services {online}/{} online",
        describe(&state.tfl_status, |status| format!(
            "{} lines",
            status.lines.len()
        )),
        describe(&state.tfl_planned, |planned| format!(
            "{} disruptions",
            planned.disruptions.len()
        )),
        describe(&state.forecast, |forecast| format!(
            "{:.1}°C",
            forecast.now.temp_c
        )),
        state.services.len()
    )
}
