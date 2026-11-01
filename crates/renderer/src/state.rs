//! The latest data from each topic the dashboard draws from.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nerve_core::envelope::{Envelope, Payload};
use nerve_core::payloads::service::ServiceStatus;
use nerve_core::payloads::tfl::{TflPlanned, TflStatus};
use nerve_core::payloads::weather::Forecast;
use nerve_core::topics::Topics;

#[derive(Debug, Clone, PartialEq)]
pub struct Received<T> {
    pub data: T,
    pub ts: DateTime<Utc>,
}

impl<T: Payload> Received<T> {
    fn decode(payload: &[u8]) -> Result<Self, serde_json::Error> {
        let envelope = Envelope::<T>::from_json(payload)?;
        Ok(Self {
            data: envelope.data,
            ts: envelope.ts,
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Updated,
    Older,
    Ignored,
}

pub struct State {
    pub tfl_status: Option<Received<TflStatus>>,
    pub tfl_planned: Option<Received<TflPlanned>>,
    pub forecast: Option<Received<Forecast>>,
    pub services: HashMap<String, ServiceStatus>,
    topics: StateTopics,
}

struct StateTopics {
    tfl_status: String,
    tfl_planned: String,
    forecast: String,
    service_prefix: String,
}

impl State {
    pub fn new(topics: &Topics) -> Self {
        Self {
            tfl_status: None,
            tfl_planned: None,
            forecast: None,
            services: HashMap::new(),
            topics: StateTopics {
                tfl_status: topics.tfl_status(),
                tfl_planned: topics.tfl_planned(),
                forecast: topics.weather_forecast(),
                service_prefix: topics.service(""),
            },
        }
    }

    pub fn filters(topics: &Topics) -> Vec<String> {
        vec![
            topics.tfl_status(),
            topics.tfl_planned(),
            topics.weather_forecast(),
            topics.all_services(),
        ]
    }

    pub fn apply(&mut self, topic: &str, payload: &[u8]) -> Result<Outcome, serde_json::Error> {
        let topics = &self.topics;

        if topic == topics.tfl_status {
            Ok(keep_newest(
                &mut self.tfl_status,
                Received::decode(payload)?,
            ))
        } else if topic == topics.tfl_planned {
            Ok(keep_newest(
                &mut self.tfl_planned,
                Received::decode(payload)?,
            ))
        } else if topic == topics.forecast {
            Ok(keep_newest(&mut self.forecast, Received::decode(payload)?))
        } else if let Some(name) = topic.strip_prefix(&topics.service_prefix) {
            let status = Envelope::<ServiceStatus>::from_json(payload)?;
            self.services.insert(name.to_string(), status.data);
            Ok(Outcome::Updated)
        } else {
            Ok(Outcome::Ignored)
        }
    }
}

fn keep_newest<T>(slot: &mut Option<Received<T>>, new: Received<T>) -> Outcome {
    if let Some(current) = slot
        && new.ts < current.ts
    {
        return Outcome::Older;
    }
    *slot = Some(new);
    Outcome::Updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use nerve_core::payloads::service::ServiceState;
    use nerve_core::payloads::tfl::{Level, Line};

    fn topics() -> Topics {
        Topics::new("home").unwrap()
    }

    /// An envelope as JSON, published at `ts`.
    fn message<T: Payload>(data: T, ts: &str) -> Vec<u8> {
        let mut envelope = Envelope::new("test", data);
        envelope.ts = ts.parse().unwrap();
        envelope.to_json().unwrap()
    }

    fn status(level: Level) -> TflStatus {
        TflStatus {
            lines: vec![Line {
                id: "victoria".into(),
                name: "Victoria".into(),
                mode: "tube".into(),
                level,
                statuses: vec![],
            }],
        }
    }

    #[test]
    fn keeps_each_topic_in_its_own_place() {
        let mut state = State::new(&topics());
        let planned = TflPlanned {
            from: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 11, 7).unwrap(),
            disruptions: vec![],
        };
        let online = ServiceStatus {
            state: ServiceState::Online,
            last_success: None,
        };

        let outcomes = [
            state.apply(
                "home/tfl/status",
                &message(status(Level::Good), "2026-11-01T08:00:00Z"),
            ),
            state.apply(
                "home/tfl/planned",
                &message(planned.clone(), "2026-11-01T07:30:00Z"),
            ),
            state.apply(
                "home/service/poller-tfl",
                &message(online.clone(), "2026-11-01T08:01:00Z"),
            ),
        ];

        for outcome in outcomes {
            assert_eq!(outcome.unwrap(), Outcome::Updated);
        }
        let held = state.tfl_status.unwrap();
        assert_eq!(held.data, status(Level::Good));
        assert_eq!(
            held.ts,
            "2026-11-01T08:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(state.tfl_planned.unwrap().data, planned);
        assert_eq!(state.services["poller-tfl"], online);
        assert!(state.forecast.is_none());
    }

    #[test]
    fn newer_data_replaces_older() {
        let mut state = State::new(&topics());
        state
            .apply(
                "home/tfl/status",
                &message(status(Level::Good), "2026-11-01T08:00:00Z"),
            )
            .unwrap();

        let outcome = state.apply(
            "home/tfl/status",
            &message(status(Level::Severe), "2026-11-01T08:05:00Z"),
        );

        assert_eq!(outcome.unwrap(), Outcome::Updated);
        assert_eq!(state.tfl_status.unwrap().data, status(Level::Severe));
    }

    #[test]
    fn older_data_is_dropped() {
        let mut state = State::new(&topics());
        state
            .apply(
                "home/tfl/status",
                &message(status(Level::Severe), "2026-11-01T08:05:00Z"),
            )
            .unwrap();

        let outcome = state.apply(
            "home/tfl/status",
            &message(status(Level::Good), "2026-11-01T08:00:00Z"),
        );

        assert_eq!(outcome.unwrap(), Outcome::Older);
        assert_eq!(state.tfl_status.unwrap().data, status(Level::Severe));
    }

    #[test]
    fn a_malformed_message_leaves_the_state_unchanged() {
        let mut state = State::new(&topics());
        let good = message(status(Level::Good), "2026-11-01T08:00:00Z");
        state.apply("home/tfl/status", &good).unwrap();

        // Not JSON, and an envelope whose data is the wrong shape.
        let wrong_shape = message(
            ServiceStatus {
                state: ServiceState::Online,
                last_success: None,
            },
            "2026-11-01T09:00:00Z",
        );
        for payload in [&b"hello"[..], &wrong_shape] {
            assert!(state.apply("home/tfl/status", payload).is_err());
        }

        assert_eq!(state.tfl_status.unwrap().data, status(Level::Good));
    }

    #[test]
    fn ignores_other_topics() {
        let mut state = State::new(&topics());
        let payload = message(status(Level::Good), "2026-11-01T08:00:00Z");

        for topic in ["home/node/nerve-centre/metrics", "dev/tfl/status"] {
            assert_eq!(state.apply(topic, &payload).unwrap(), Outcome::Ignored);
        }
        assert!(state.tfl_status.is_none());
    }
}
