use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use chrono_tz::Tz;
use nerve_core::payloads::weather::{Condition, Forecast, Hour, Now, Today};
use serde::Deserialize;
use thiserror::Error;

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

pub struct Api {
    http: reqwest::Client,
    query: Vec<(&'static str, String)>,
    location: String,
    timezone: Tz,
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected response: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unexpected response: {0}")]
    Invalid(&'static str),
}

impl Api {
    pub fn new(
        location: String,
        latitude: f64,
        longitude: f64,
        timezone: Tz,
    ) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("nerve-centre")
            .build()?;

        // Times as Unix timestamps, converted to local time by us, so
        // clock-change days come out right.
        let query = vec![
            ("latitude", latitude.to_string()),
            ("longitude", longitude.to_string()),
            ("timezone", timezone.name().to_string()),
            ("timeformat", "unixtime".to_string()),
            ("wind_speed_unit", "mph".to_string()),
            ("forecast_days", "1".to_string()),
            (
                "current",
                "temperature_2m,apparent_temperature,weather_code,precipitation,wind_speed_10m,wind_gusts_10m"
                    .to_string(),
            ),
            ("hourly", "temperature_2m,weather_code,precipitation_probability".to_string()),
            ("daily", "temperature_2m_max,temperature_2m_min,sunrise,sunset".to_string()),
        ];

        Ok(Self {
            http,
            query,
            location,
            timezone,
        })
    }

    pub async fn fetch(&self) -> Result<Forecast, FetchError> {
        let response = self
            .http
            .get(FORECAST_URL)
            .query(&self.query)
            .send()
            .await?
            .error_for_status()?;
        let body = response.bytes().await?;
        parse_forecast(&body, &self.location, self.timezone)
    }
}

/// Open_meteo's response
#[derive(Debug, Deserialize)]
struct ApiForecast {
    current: ApiCurrent,
    hourly: ApiHourly,
    daily: ApiDaily,
}

#[derive(Debug, Deserialize)]
struct ApiCurrent {
    time: i64,
    temperature_2m: f64,
    apparent_temperature: f64,
    weather_code: u8,
    precipitation: f64,
    wind_speed_10m: f64,
    wind_gusts_10m: f64,
}

#[derive(Debug, Deserialize)]
struct ApiHourly {
    time: Vec<i64>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<u8>,
    precipitation_probability: Vec<Option<u8>>,
}

#[derive(Debug, Deserialize)]
struct ApiDaily {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    sunrise: Vec<i64>,
    sunset: Vec<i64>,
}

pub fn parse_forecast(json: &[u8], location: &str, timezone: Tz) -> Result<Forecast, FetchError> {
    let api: ApiForecast = serde_json::from_slice(json)?;
    let current = api.current;

    let now = Now {
        temp_c: current.temperature_2m,
        feels_like_c: current.apparent_temperature,
        condition: condition_for(current.weather_code),
        precip_mm: current.precipitation,
        wind_mph: current.wind_speed_10m,
        gust_mph: current.wind_gusts_10m,
    };

    let hourly = api.hourly;
    let rows = hourly
        .time
        .into_iter()
        .zip(hourly.temperature_2m)
        .zip(hourly.weather_code)
        .zip(hourly.precipitation_probability);

    let mut hours = Vec::new();
    for (((time, temp_c), code), rain_chance) in rows {
        if time <= current.time {
            continue;
        }
        hours.push(Hour {
            time: local_time(time, timezone)?,
            temp_c,
            condition: condition_for(code),
            rain_chance: rain_chance.unwrap_or(0),
        });
    }

    let daily = api.daily;
    let today = Today {
        high_c: first(&daily.temperature_2m_max)?,
        low_c: first(&daily.temperature_2m_min)?,
        rain_chance_max: hours.iter().map(|h| h.rain_chance).max().unwrap_or(0),
        sunrise: local_time(first(&daily.sunrise)?, timezone)?,
        sunset: local_time(first(&daily.sunset)?, timezone)?,
    };

    Ok(Forecast {
        location: location.to_string(),
        now,
        today,
        hours,
    })
}

fn first<T: Copy>(values: &[T]) -> Result<T, FetchError> {
    values
        .first()
        .copied()
        .ok_or(FetchError::Invalid("daily values missing"))
}

fn local_time(unix_seconds: i64, timezone: Tz) -> Result<DateTime<FixedOffset>, FetchError> {
    let utc = DateTime::from_timestamp(unix_seconds, 0)
        .ok_or(FetchError::Invalid("time out of range"))?;
    Ok(utc.with_timezone(&timezone).fixed_offset())
}

/// Maps a WMO weather code to our condition. See the table in the roadmap.
fn condition_for(code: u8) -> Condition {
    match code {
        0 | 1 => Condition::Clear,
        2 => Condition::PartlyCloudy,
        3 => Condition::Cloudy,
        45 | 48 => Condition::Fog,
        51 | 53 | 55 | 56 | 57 => Condition::Drizzle,
        61 | 63 | 66 | 80 | 81 => Condition::Rain,
        65 | 67 | 82 => Condition::HeavyRain,
        71 | 73 | 75 | 77 | 85 | 86 => Condition::Snow,
        95 | 96 | 99 => Condition::Thunderstorm,
        _ => Condition::Cloudy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONDON: Tz = chrono_tz::Europe::London;

    #[test]
    fn parses_a_real_response() {
        let json = include_bytes!("../fixtures/forecast.json");
        let forecast = parse_forecast(json, "London", LONDON).unwrap();

        assert_eq!(forecast.location, "London");
        assert!(forecast.today.sunrise < forecast.today.sunset);
        assert!(
            forecast
                .hours
                .windows(2)
                .all(|pair| pair[0].time < pair[1].time)
        );
    }

    #[test]
    fn keeps_later_hours_in_local_time() {
        // Now is 19:15 UTC, 20:15 in London (BST).
        let json = br#"{
            "current": { "time": 1792264500, "temperature_2m": 12.5, "apparent_temperature": 10.1,
                         "weather_code": 61, "precipitation": 0.4,
                         "wind_speed_10m": 11.0, "wind_gusts_10m": 24.0 },
            "hourly": {
                "time": [1792260000, 1792263600, 1792267200, 1792270800],
                "temperature_2m": [13.0, 12.6, 12.1, 11.4],
                "weather_code": [0, 61, 3, 95],
                "precipitation_probability": [10, 90, 30, null]
            },
            "daily": { "temperature_2m_max": [14.0], "temperature_2m_min": [7.5],
                       "sunrise": [1792218420], "sunset": [1792256580] }
        }"#;
        let forecast = parse_forecast(json, "London", LONDON).unwrap();

        assert_eq!(forecast.now.condition, Condition::Rain);

        // Only 21:00 and 22:00 are after 20:15.
        let [nine, ten] = forecast.hours.as_slice() else {
            panic!("expected two hours, got {:?}", forecast.hours)
        };
        assert_eq!(nine.time.to_rfc3339(), "2026-10-17T21:00:00+01:00");
        assert_eq!(nine.condition, Condition::Cloudy);
        assert_eq!(ten.condition, Condition::Thunderstorm);
        assert_eq!(ten.rain_chance, 0);

        // The 90% belongs to an hour that has passed.
        assert_eq!(forecast.today.rain_chance_max, 30);
        assert_eq!(
            forecast.today.sunset.to_rfc3339(),
            "2026-10-17T18:03:00+01:00"
        );
    }

    #[test]
    fn handles_the_clocks_going_back() {
        // On 25 October 2026, 01:00 to 02:00 happens twice in London.
        let before = local_time(1792888200, LONDON).unwrap();
        let after = local_time(1792891800, LONDON).unwrap();

        assert_eq!(before.to_rfc3339(), "2026-10-25T01:30:00+01:00");
        assert_eq!(after.to_rfc3339(), "2026-10-25T01:30:00+00:00");
    }

    #[test]
    fn maps_codes_to_conditions() {
        assert_eq!(condition_for(1), Condition::Clear);
        assert_eq!(condition_for(48), Condition::Fog);
        assert_eq!(condition_for(82), Condition::HeavyRain);
        assert_eq!(condition_for(250), Condition::Cloudy);
    }
}
