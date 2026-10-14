//! Service health. Published to `service/{name}`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::envelope::Payload;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub state: ServiceState,
    /// When the service last fetched successfully. `None` if it hasn't
    /// since starting, and always `None` in a Last Will.
    pub last_success: Option<DateTime<Utc>>,
}

impl Payload for ServiceStatus {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Online,
    Offline,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn last_will_matches_the_documented_json() {
        let last_will = ServiceStatus {
            state: ServiceState::Offline,
            last_success: None,
        };
        assert_eq!(
            serde_json::to_value(&last_will).unwrap(),
            json!({ "state": "offline", "last_success": null })
        );
    }
}
