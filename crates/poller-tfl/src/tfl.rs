//! TfL's line status API: fetching, response format and conversion into
//! our payloads.

use std::time::Duration;

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use nerve_core::payloads::tfl::{
    Level, Line, LineStatus, Period, PlannedDisruption, TflPlanned, TflStatus,
};
use serde::Deserialize;
use thiserror::Error;

const BASE_URL: &str = "https://api.tfl.gov.uk/Line";

pub struct Api {
    http: reqwest::Client,
    app_key: Option<String>,
    line_ids: String,
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("request failed: {0}")]
    Http(reqwest::Error),
    #[error("unexpected response: {0}")]
    Parse(#[from] serde_json::Error),
}

impl From<reqwest::Error> for FetchError {
    fn from(error: reqwest::Error) -> Self {
        FetchError::Http(error.without_url())
    }
}

impl Api {
    pub fn new(app_key: Option<String>, lines: &[String]) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("nerve-centre")
            .build()?;
        Ok(Self {
            http,
            app_key,
            line_ids: lines.join(","),
        })
    }

    pub async fn fetch_status(&self) -> Result<TflStatus, FetchError> {
        let body = self
            .get(&format!("{BASE_URL}/{}/Status", self.line_ids))
            .await?;
        Ok(parse_status(&body)?)
    }

    pub async fn fetch_planned(&self, days: u32) -> Result<TflPlanned, FetchError> {
        let from = Local::now().date_naive();
        let to = from + Days::new(days.into());
        let url = format!("{BASE_URL}/{}/Status/{from}/to/{to}", self.line_ids);

        let body = self.get(&url).await?;
        Ok(parse_planned(&body, from, to)?)
    }

    async fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let mut request = self.http.get(url);
        if let Some(key) = &self.app_key {
            request = request.query(&[("app_key", key)]);
        }

        let response = request.send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }
}

/// One line as TfL returns it. Only the fields we use; serde skips the rest.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiLine {
    id: String,
    name: String,
    mode_name: String,
    line_statuses: Vec<ApiLineStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiLineStatus {
    status_severity: u8,
    status_severity_description: String,
    /// Missing for Good Service.
    reason: Option<String>,
    #[serde(default)]
    validity_periods: Vec<ApiPeriod>,
    disruption: Option<ApiDisruption>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiPeriod {
    from_date: DateTime<Utc>,
    to_date: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ApiDisruption {
    /// e.g. `PlannedWork`, `RealTime` or `Information`.
    category: String,
}

pub fn parse_status(json: &[u8]) -> serde_json::Result<TflStatus> {
    let api_lines: Vec<ApiLine> = serde_json::from_slice(json)?;

    let mut lines: Vec<Line> = api_lines.into_iter().map(to_line).collect();
    // Sort so a change in TfL's ordering isn't mistaken for a change in status.
    lines.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(TflStatus { lines })
}

pub fn parse_planned(
    json: &[u8],
    from: NaiveDate,
    to: NaiveDate,
) -> serde_json::Result<TflPlanned> {
    let api_lines: Vec<ApiLine> = serde_json::from_slice(json)?;

    let mut disruptions = Vec::new();
    for line in api_lines {
        for status in line.line_statuses {
            let is_planned = status
                .disruption
                .as_ref()
                .is_some_and(|d| d.category == "PlannedWork");
            if !is_planned {
                continue;
            }

            disruptions.push(PlannedDisruption {
                line_id: line.id.clone(),
                level: level_for(status.status_severity),
                description: status.status_severity_description,
                detail: status.reason.unwrap_or_default().trim().to_string(),
                periods: status
                    .validity_periods
                    .into_iter()
                    .map(|p| Period {
                        starts: p.from_date,
                        ends: p.to_date,
                    })
                    .collect(),
            });
        }
    }

    disruptions.sort_by_key(|d| (d.line_id.clone(), d.periods.first().map(|p| p.starts)));
    Ok(TflPlanned {
        from,
        to,
        disruptions,
    })
}

fn to_line(api: ApiLine) -> Line {
    let level = api
        .line_statuses
        .iter()
        .map(|s| level_for(s.status_severity))
        .max()
        .unwrap_or(Level::Unknown);

    let statuses = api
        .line_statuses
        .into_iter()
        .map(|s| LineStatus {
            code: s.status_severity,
            description: s.status_severity_description,
            reason: s
                .reason
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty()),
        })
        .collect();

    Line {
        id: api.id,
        name: api.name,
        mode: api.mode_name,
        level,
        statuses,
    }
}

/// Maps a TfL severity code to our 'level'
fn level_for(code: u8) -> Level {
    match code {
        10 | 13 | 18 | 19 => Level::Good,
        0 | 7 | 9 | 12 | 14 | 15 | 17 => Level::Minor,
        3 | 5 | 6 | 8 | 11 => Level::Severe,
        1 | 2 | 4 | 16 | 20 => Level::Closed,
        _ => Level::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_tfl_response() {
        let json = include_bytes!("../fixtures/line-status.json");
        let status = parse_status(json).unwrap();

        assert_eq!(status.lines.len(), 12);
        let elizabeth = status.lines.iter().find(|l| l.id == "elizabeth").unwrap();
        assert_eq!(elizabeth.mode, "elizabeth-line");
        assert!(status.lines.iter().all(|l| !l.statuses.is_empty()));
    }

    #[test]
    fn uses_the_worst_status_and_tidies_reasons() {
        let json = br#"[
            { "id": "jubilee", "name": "Jubilee", "modeName": "tube", "lineStatuses": [
                { "statusSeverity": 9, "statusSeverityDescription": "Minor Delays", "reason": "Signal failure. " },
                { "statusSeverity": 3, "statusSeverityDescription": "Part Suspended", "reason": "No service to Stanmore." }
            ]},
            { "id": "bakerloo", "name": "Bakerloo", "modeName": "tube", "lineStatuses": [
                { "statusSeverity": 10, "statusSeverityDescription": "Good Service" }
            ]}
        ]"#;
        let status = parse_status(json).unwrap();

        // Sorted by ID, so Bakerloo comes first.
        let [bakerloo, jubilee] = status.lines.as_slice() else {
            panic!("expected two lines")
        };

        assert_eq!(bakerloo.level, Level::Good);
        assert_eq!(bakerloo.statuses[0].reason, None);

        assert_eq!(jubilee.level, Level::Severe);
        assert_eq!(
            jubilee.statuses[0].reason.as_deref(),
            Some("Signal failure.")
        );
    }

    #[test]
    fn maps_codes_to_levels() {
        assert_eq!(level_for(10), Level::Good);
        assert_eq!(level_for(9), Level::Minor);
        assert_eq!(level_for(6), Level::Severe);
        assert_eq!(level_for(20), Level::Closed);
        assert_eq!(level_for(99), Level::Unknown);
    }

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn parses_a_real_planned_response() {
        let json = include_bytes!("../fixtures/line-status-planned.json");
        let planned = parse_planned(json, date("2026-10-17"), date("2026-10-24")).unwrap();

        assert!(!planned.disruptions.is_empty());
        assert!(planned.disruptions.iter().all(|d| !d.periods.is_empty()));
    }

    #[test]
    fn keeps_only_planned_works_with_all_their_periods() {
        let json = br#"[
            { "id": "elizabeth", "name": "Elizabeth line", "modeName": "elizabeth-line", "lineStatuses": [
                { "statusSeverity": 7, "statusSeverityDescription": "Reduced Service",
                  "reason": "Late evening changes. ",
                  "validityPeriods": [
                      { "fromDate": "2026-10-19T22:00:00Z", "toDate": "2026-10-19T22:59:00Z" },
                      { "fromDate": "2026-10-20T22:00:00Z", "toDate": "2026-10-20T22:59:00Z" }
                  ],
                  "disruption": { "category": "PlannedWork" } },
                { "statusSeverity": 9, "statusSeverityDescription": "Minor Delays",
                  "disruption": { "category": "RealTime" } }
            ]},
            { "id": "waterloo-city", "name": "Waterloo & City", "modeName": "tube", "lineStatuses": [
                { "statusSeverity": 4, "statusSeverityDescription": "Planned Closure",
                  "disruption": { "category": "Information" } }
            ]},
            { "id": "bakerloo", "name": "Bakerloo", "modeName": "tube", "lineStatuses": [
                { "statusSeverity": 10, "statusSeverityDescription": "Good Service" }
            ]}
        ]"#;
        let planned = parse_planned(json, date("2026-10-17"), date("2026-10-24")).unwrap();

        let [elizabeth] = planned.disruptions.as_slice() else {
            panic!("expected one disruption, got {:?}", planned.disruptions)
        };
        assert_eq!(elizabeth.line_id, "elizabeth");
        assert_eq!(elizabeth.level, Level::Minor);
        assert_eq!(elizabeth.detail, "Late evening changes.");
        assert_eq!(elizabeth.periods.len(), 2);
    }
}
