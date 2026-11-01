//! A node's resource use. Published to `node/{name}/metrics` every
//! interval, whether or not anything changed.

use serde::{Deserialize, Serialize};

use crate::envelope::Payload;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub interval_s: u64,
    pub cpu_pct: f64,
    pub load_1m: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temp_c: Option<f64>,
    pub throttle: Option<Throttle>,
    pub disk_used_pct: f64,
    pub uptime_s: u64,
    pub services: Vec<ServiceMetrics>,
}

impl Payload for NodeMetrics {
    const VERSION: u32 = 1;
}

/// The Raspberry Pi firmware's throttling flags. An empty list means
/// all is well.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Throttle {
    pub now: Vec<ThrottleFlag>,
    pub since_boot: Vec<ThrottleFlag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThrottleFlag {
    UnderVoltage,
    FreqCapped,
    Throttled,
    SoftTempLimit,
}

/// One systemd unit's resource use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub name: String,
    pub mem_mb: Option<f64>,
    // Average over the interval, as a share of the whole node
    pub cpu_pct: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_the_documented_json() {
        let metrics = NodeMetrics {
            interval_s: 60,
            cpu_pct: 0.1,
            load_1m: 1.0,
            mem_used_mb: 280,
            mem_total_mb: 4049,
            temp_c: Some(49.1),
            throttle: Some(Throttle {
                now: vec![],
                since_boot: vec![ThrottleFlag::UnderVoltage],
            }),
            disk_used_pct: 1.8,
            uptime_s: 901640,
            services: vec![ServiceMetrics {
                name: "mosquitto".to_string(),
                mem_mb: Some(4.9),
                cpu_pct: None,
            }],
        };

        assert_eq!(
            serde_json::to_value(&metrics).unwrap(),
            json!({
                "interval_s": 60,
                "cpu_pct": 0.1, "load_1m": 1.0,
                "mem_used_mb": 280, "mem_total_mb": 4049,
                "temp_c": 49.1,
                "throttle": { "now": [], "since_boot": ["under_voltage"] },
                "disk_used_pct": 1.8, "uptime_s": 901640,
                "services": [ { "name": "mosquitto", "mem_mb": 4.9, "cpu_pct": null } ]
            })
        );
    }
}
