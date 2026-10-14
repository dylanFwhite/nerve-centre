//! TfL line status: current and planned.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::envelope::Payload;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TflStatus {
    pub lines: Vec<Line>,
}

impl Payload for TflStatus {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// TfL's line ID, e.g. `victoria` or `elizabeth`.
    pub id: String,
    /// Display name, e.g. `Victoria`.
    pub name: String,
    /// TfL's mode, e.g. `tube` or `elizabeth-line`.
    pub mode: String,
    /// Our overall rating for the line: the worst of its statuses.
    pub level: Level,
    /// TfL's statuses, as received. A line can have several at once.
    pub statuses: Vec<LineStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStatus {
    /// TfL's severity code, e.g. 10 for Good Service.
    pub code: u8,
    /// TfL's description, e.g. `Good Service` or `Minor Delays`.
    pub description: String,
    /// TfL's explanation, when there is one.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Good,
    Unknown,
    Minor,
    Severe,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TflPlanned {
    /// First day covered, London date.
    pub from: NaiveDate,
    /// Last day covered, London date.
    pub to: NaiveDate,
    pub disruptions: Vec<PlannedDisruption>,
}

impl Payload for TflPlanned {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedDisruption {
    pub line_id: String,
    /// TfL's description, e.g. `Planned Closure`.
    pub description: String,
    /// TfL's full explanation of what is affected.
    pub detail: String,
    pub starts: DateTime<Utc>,
    pub ends: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_matches_the_documented_json() {
        let status = TflStatus {
            lines: vec![Line {
                id: "victoria".into(),
                name: "Victoria".into(),
                mode: "tube".into(),
                level: Level::Good,
                statuses: vec![LineStatus {
                    code: 10,
                    description: "Good Service".into(),
                    reason: None,
                }],
            }],
        };

        let expected = json!({
            "lines": [{
                "id": "victoria", "name": "Victoria", "mode": "tube", "level": "good",
                "statuses": [{ "code": 10, "description": "Good Service", "reason": null }]
            }]
        });
        assert_eq!(serde_json::to_value(&status).unwrap(), expected);
    }

    #[test]
    fn the_worst_level_wins() {
        let levels = [Level::Good, Level::Severe, Level::Minor];
        assert_eq!(levels.into_iter().max(), Some(Level::Severe));
        assert!(Level::Unknown > Level::Good);
        assert!(Level::Unknown < Level::Minor);
    }

    #[test]
    fn planned_round_trips_through_json() {
        let planned = TflPlanned {
            from: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
            disruptions: vec![PlannedDisruption {
                line_id: "district".into(),
                description: "Planned Closure".into(),
                detail: "No service between Earl's Court and Richmond".into(),
                starts: "2026-10-18T04:30:00Z".parse().unwrap(),
                ends: "2026-10-19T23:59:00Z".parse().unwrap(),
            }],
        };

        let value = serde_json::to_value(&planned).unwrap();
        assert_eq!(value["from"], "2026-10-13");

        let decoded: TflPlanned = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, planned);
    }
}
