//! Decides what to log for each message, and logs it.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nerve_core::envelope::Envelope;
use nerve_core::payloads::service::{ServiceState, ServiceStatus};
use serde::Deserialize;
use uuid::Uuid;

use crate::store::{Event, Store, StoreError};

#[derive(Deserialize)]
struct Header {
    id: Uuid,
    ts: DateTime<Utc>,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Written to the log
    Stored,
    /// Already in the log: a redelivery or a retained resend.
    Duplicate,
    /// A service heartbeat that doesn't change the service's state.
    Unchanged,
    /// An invalid envelope
    Invalid(String),
}

pub struct Recorder {
    store: Store,
    service_prefix: String,
    /// The last state logged for each service
    services: HashMap<String, ServiceState>,
}

impl Recorder {
    pub fn new(store: Store, service_prefix: String) -> Result<Self, StoreError> {
        let mut services = HashMap::new();
        for (topic, payload) in store.latest_by_topic(&service_prefix)? {
            if let Ok(envelope) = Envelope::<ServiceStatus>::from_json(payload.as_bytes()) {
                services.insert(topic, envelope.data.state);
            }
        }

        Ok(Self {
            store,
            service_prefix,
            services,
        })
    }

    pub fn record(
        &mut self,
        topic: &str,
        payload: &[u8],
        received_at: DateTime<Utc>,
    ) -> Result<Outcome, StoreError> {
        let Ok(text) = std::str::from_utf8(payload) else {
            return Ok(Outcome::Invalid("not UTF-8".to_string()));
        };

        if topic.starts_with(&self.service_prefix) {
            return self.record_service(topic, text, received_at);
        }

        match serde_json::from_str::<Header>(text) {
            Ok(header) => self.insert(topic, header, text, received_at),
            Err(error) => Ok(Outcome::Invalid(error.to_string())),
        }
    }

    fn record_service(
        &mut self,
        topic: &str,
        text: &str,
        received_at: DateTime<Utc>,
    ) -> Result<Outcome, StoreError> {
        let envelope = match Envelope::<ServiceStatus>::from_json(text.as_bytes()) {
            Ok(envelope) => envelope,
            Err(error) => return Ok(Outcome::Invalid(error.to_string())),
        };

        let state = envelope.data.state;
        if self.services.get(topic) == Some(&state) {
            return Ok(Outcome::Unchanged);
        }

        let header = Header {
            id: envelope.id,
            ts: envelope.ts,
        };
        let outcome = self.insert(topic, header, text, received_at)?;
        self.services.insert(topic.to_string(), state);

        Ok(outcome)
    }

    fn insert(
        &self,
        topic: &str,
        header: Header,
        text: &str,
        received_at: DateTime<Utc>,
    ) -> Result<Outcome, StoreError> {
        let id = header.id.to_string();
        let event = Event {
            id: &id,
            topic,
            ts: header.ts,
            received_at,
            payload: text,
        };

        if self.store.insert(&event)? {
            Ok(Outcome::Stored)
        } else {
            Ok(Outcome::Duplicate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TFL: &str = "home/service/poller-tfl";

    fn recorder() -> Recorder {
        Recorder::new(
            Store::open_in_memory().unwrap(),
            "home/service/".to_string(),
        )
        .unwrap()
    }

    fn status(state: ServiceState) -> Vec<u8> {
        let status = ServiceStatus {
            state,
            last_success: None,
        };
        Envelope::new("poller-tfl", status).to_json().unwrap()
    }

    #[test]
    fn stores_a_message_once() {
        let mut recorder = recorder();
        let message = json!({
            "id": Uuid::now_v7(),
            "ts": "2026-10-27T08:00:00Z",
            "source": "poller-weather",
            "v": 1,
            "data": { "anything": "at all" }
        })
        .to_string();

        let first = recorder.record("home/weather/forecast", message.as_bytes(), Utc::now());
        let again = recorder.record("home/weather/forecast", message.as_bytes(), Utc::now());

        assert_eq!(first.unwrap(), Outcome::Stored);
        assert_eq!(again.unwrap(), Outcome::Duplicate);
    }

    #[test]
    fn skips_messages_that_are_not_envelopes() {
        let mut recorder = recorder();
        let not_utf8 = [0xff, 0xfe];
        let not_json = b"hello";
        let no_id = br#"{ "ts": "2026-10-27T08:00:00Z", "data": {} }"#;

        for payload in [&not_utf8[..], not_json, no_id] {
            let outcome = recorder
                .record("home/tfl/status", payload, Utc::now())
                .unwrap();
            assert!(matches!(outcome, Outcome::Invalid(_)), "{outcome:?}");
        }
    }

    #[test]
    fn stores_only_service_state_changes() {
        let mut recorder = recorder();
        let mut record = |state| recorder.record(TFL, &status(state), Utc::now()).unwrap();

        assert_eq!(record(ServiceState::Online), Outcome::Stored);
        assert_eq!(record(ServiceState::Online), Outcome::Unchanged);
        assert_eq!(record(ServiceState::Offline), Outcome::Stored);
        assert_eq!(record(ServiceState::Online), Outcome::Stored);
    }

    #[test]
    fn tracks_each_service_separately() {
        let mut recorder = recorder();
        let tfl_online = status(ServiceState::Online);
        let weather_online = status(ServiceState::Online);

        let tfl = recorder.record(TFL, &tfl_online, Utc::now()).unwrap();
        let weather = recorder
            .record("home/service/poller-weather", &weather_online, Utc::now())
            .unwrap();

        assert_eq!(tfl, Outcome::Stored);
        assert_eq!(weather, Outcome::Stored);
    }

    #[test]
    fn remembers_service_states_from_the_store() {
        // A store that already logged poller-tfl as online, as after a restart.
        let store = Store::open_in_memory().unwrap();
        let id = Uuid::now_v7().to_string();
        let online = String::from_utf8(status(ServiceState::Online)).unwrap();
        let earlier = Event {
            id: &id,
            topic: TFL,
            ts: Utc::now(),
            received_at: Utc::now(),
            payload: &online,
        };
        store.insert(&earlier).unwrap();

        let mut recorder = Recorder::new(store, "home/service/".to_string()).unwrap();
        let outcome = recorder
            .record(TFL, &status(ServiceState::Online), Utc::now())
            .unwrap();

        assert_eq!(outcome, Outcome::Unchanged);
    }
}
