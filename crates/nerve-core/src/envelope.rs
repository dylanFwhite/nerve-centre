use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

pub trait Payload: Serialize + DeserializeOwned + PartialEq {
    const VERSION: u32;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub source: String,
    pub v: u32,
    pub data: T,
}

impl<T: Payload> Envelope<T> {
    pub fn new(source: impl Into<String>, data: T) -> Self {
        Self {
            id: Uuid::now_v7(),
            ts: Utc::now(),
            source: source.into(),
            v: T::VERSION,
            data,
        }
    }

    pub fn to_json(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    pub fn from_json(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Ping {
        message: String,
    }

    impl Payload for Ping {
        const VERSION: u32 = 3;
    }

    #[test]
    fn round_trips_through_json() {
        let original = Envelope::new(
            "test",
            Ping {
                message: "hello".into(),
            },
        );
        let bytes = original.to_json().unwrap();
        let decoded = Envelope::<Ping>::from_json(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn version_comes_from_the_payload_type() {
        let envelope = Envelope::new(
            "test",
            Ping {
                message: "hello".into(),
            },
        );
        assert_eq!(envelope.v, 3);
    }
}
