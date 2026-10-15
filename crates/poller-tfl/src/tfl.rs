//! TfL's line status API: response format and conversion into our payloads.

use nerve_core::payloads::tfl::{Level, Line, LineStatus, TflStatus};
use serde::Deserialize;

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
}

pub fn parse_status(json: &[u8]) -> serde_json::Result<TflStatus> {
    let api_lines: Vec<ApiLine> = serde_json::from_slice(json)?;

    let mut lines: Vec<Line> = api_lines.into_iter().map(to_line).collect();
    // Sort so a change in TfL's ordering isn't mistaken for a change in status.
    lines.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(TflStatus { lines })
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
}
