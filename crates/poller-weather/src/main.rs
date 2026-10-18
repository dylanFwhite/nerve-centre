mod config;
mod weather;

use std::error::Error;
use std::path::PathBuf;
use std::pin::pin;

use chrono::{DateTime, Utc};
use nerve_core::mqtt::Publisher;
use nerve_core::poll::{PublishOnChange, ticker};
use nerve_core::shutdown::shutdown_signal;

const SERVICE: &str = "poller-weather";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config/dev.toml".into()),
    );
    let config: config::Config = nerve_core::config::load(&path)?;
    let settings = config.weather;

    let api = weather::Api::new(
        settings.location,
        settings.latitude,
        settings.longitude,
        settings.timezone,
    )?;
    let publisher = Publisher::connect(&config.mqtt, SERVICE)?;

    let mut forecast_ticker = ticker(settings.poll_seconds);
    let mut forecast_out = PublishOnChange::new(publisher.topics().weather_forecast());

    let mut shutdown = pin!(shutdown_signal());
    let mut last_success: Option<DateTime<Utc>> = None;

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => break,

            _ = forecast_ticker.tick() => {
                match api.fetch().await {
                    Ok(forecast) => {
                        last_success = Some(Utc::now());
                        forecast_out.publish(&publisher, forecast).await?;
                    }
                    Err(error) => eprintln!("weather: {error}"),
                }
                publisher.report_online(last_success).await?;
            }
        }
    }

    println!("shutting down");
    publisher.disconnect(last_success).await?;
    Ok(())
}
