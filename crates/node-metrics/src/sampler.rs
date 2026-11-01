//! Turns successive readings into published figures.

use std::time::Instant;

use nerve_core::payloads::node::{NodeMetrics, ServiceMetrics};

use crate::node::{ReadError, Reading};
use crate::services::{self, UnitSample};

pub struct Sample {
    pub at: Instant,
    pub node: Reading,
    pub units: Vec<UnitSample>,
}

impl Sample {
    pub fn take(units: &[String]) -> Result<Self, ReadError> {
        Ok(Self {
            at: Instant::now(),
            node: Reading::take()?,
            units: services::read(units)?,
        })
    }
}

pub struct Sampler {
    interval_s: u64,
    cores: usize,
    previous: Option<Sample>,
}

impl Sampler {
    pub fn new(interval_s: u64, cores: usize) -> Self {
        Self {
            interval_s,
            cores,
            previous: None,
        }
    }

    pub fn next(&mut self, sample: Sample) -> Option<NodeMetrics> {
        let metrics = self
            .previous
            .as_ref()
            .map(|earlier| self.metrics(earlier, &sample));
        self.previous = Some(sample);
        metrics
    }

    fn metrics(&self, earlier: &Sample, later: &Sample) -> NodeMetrics {
        let elapsed = later.at.duration_since(earlier.at);
        let node = &later.node;
        let services = services::service_metrics(&earlier.units, &later.units, elapsed, self.cores)
            .into_iter()
            .map(|service| ServiceMetrics {
                mem_mb: service.mem_mb.map(|mb| round(mb, 1)),
                cpu_pct: service.cpu_pct.map(|pct| round(pct, 2)),
                ..service
            })
            .collect();

        NodeMetrics {
            interval_s: self.interval_s,
            cpu_pct: round(node.cpu.busy_pct_since(&earlier.node.cpu), 2),
            load_1m: node.load_1m,
            mem_used_mb: node.mem_used_mb,
            mem_total_mb: node.mem_total_mb,
            temp_c: node.temp_c.map(|temp| round(temp, 1)),
            throttle: node.throttle.clone(),
            disk_used_pct: round(node.disk_used_pct, 2),
            uptime_s: node.uptime_s,
            services,
        }
    }
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10_f64.powi(places);
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::node::{CpuTimes, parse_throttled};

    fn sample(at: Instant, stat: &str, show: &str) -> Sample {
        Sample {
            at,
            node: Reading {
                cpu: CpuTimes::parse(stat).unwrap(),
                load_1m: 1.0,
                mem_used_mb: 280,
                mem_total_mb: 4049,
                temp_c: Some(47.4),
                throttle: Some(parse_throttled("throttled=0x0").unwrap()),
                disk_used_pct: 1.7865,
                uptime_s: 901865,
            },
            units: services::parse_show(show).unwrap(),
        }
    }

    #[test]
    fn the_first_sample_only_sets_a_baseline() {
        let mut sampler = Sampler::new(60, 4);
        let first = sample(
            Instant::now(),
            include_str!("../fixtures/stat-1.txt"),
            include_str!("../fixtures/systemctl-show-1.txt"),
        );

        assert!(sampler.next(first).is_none());
    }

    #[test]
    fn publishes_rounded_figures_from_two_samples() {
        let start = Instant::now();
        let mut sampler = Sampler::new(60, 4);
        sampler.next(sample(
            start,
            include_str!("../fixtures/stat-1.txt"),
            include_str!("../fixtures/systemctl-show-1.txt"),
        ));
        let metrics = sampler
            .next(sample(
                start + Duration::from_secs(5),
                include_str!("../fixtures/stat-2.txt"),
                include_str!("../fixtures/systemctl-show-2.txt"),
            ))
            .unwrap();

        assert_eq!(metrics.interval_s, 60);
        assert_eq!(metrics.cpu_pct, 0.05);
        assert_eq!(metrics.disk_used_pct, 1.79);
        assert_eq!(metrics.services.len(), 5);
        let mosquitto = &metrics.services[0];
        assert_eq!(mosquitto.mem_mb, Some(4.8));
        assert_eq!(mosquitto.cpu_pct, Some(0.0));
    }

    #[test]
    fn rounds_to_decimal_places() {
        assert_eq!(round(0.049975, 2), 0.05);
        assert_eq!(round(4.828, 1), 4.8);
        assert_eq!(round(47.0, 1), 47.0);
    }
}
