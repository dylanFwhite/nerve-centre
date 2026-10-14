//! Weather forecast for one location.

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::envelope::Payload;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    pub location: String,
    pub now: Now,
    pub today: Today,
    /// Hourly forecast from the next hour until midnight, local time.
    pub hours: Vec<Hour>,
}

impl Payload for Forecast {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Now {
    pub temp_c: f64,
    pub feels_like_c: f64,
    pub condition: Condition,
    /// Rain in the current hour, in millimetres.
    pub precip_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Today {
    pub high_c: f64,
    pub low_c: f64,
    /// Highest chance of rain in any remaining hour, as a percentage.
    pub rain_chance_max: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    /// Start of the hour, with the local UTC offset (e.g. `+01:00` in summer).
    pub time: DateTime<FixedOffset>,
    pub temp_c: f64,
    pub condition: Condition,
    /// Chance of rain, as a percentage.
    pub rain_chance: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    HeavyRain,
    Snow,
    Thunderstorm,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn forecast_matches_the_documented_json() {
        let forecast = Forecast {
            location: "London".into(),
            now: Now {
                temp_c: 14.2,
                feels_like_c: 12.8,
                condition: Condition::Drizzle,
                precip_mm: 0.3,
            },
            today: Today {
                high_c: 16.0,
                low_c: 9.5,
                rain_chance_max: 70,
            },
            hours: vec![Hour {
                time: "2026-10-12T21:00:00+01:00".parse().unwrap(),
                temp_c: 13.1,
                condition: Condition::Cloudy,
                rain_chance: 20,
            }],
        };

        let expected = json!({
            "location": "London",
            "now": { "temp_c": 14.2, "feels_like_c": 12.8, "condition": "drizzle", "precip_mm": 0.3 },
            "today": { "high_c": 16.0, "low_c": 9.5, "rain_chance_max": 70 },
            "hours": [{ "time": "2026-10-12T21:00:00+01:00", "temp_c": 13.1, "condition": "cloudy", "rain_chance": 20 }]
        });
        assert_eq!(serde_json::to_value(&forecast).unwrap(), expected);
    }

    #[test]
    fn multi_word_conditions_use_snake_case() {
        assert_eq!(
            serde_json::to_value(Condition::PartlyCloudy).unwrap(),
            "partly_cloudy"
        );
        assert_eq!(
            serde_json::to_value(Condition::HeavyRain).unwrap(),
            "heavy_rain"
        );
    }
}
