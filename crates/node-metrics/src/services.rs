//! Per-service figures from systemd's accounting, read with
//! `systemctl show`.

use std::process::Command;
use std::time::Duration;

use nerve_core::payloads::node::ServiceMetrics;

use crate::node::{ParseError, ReadError};

const MIB: f64 = 1024.0 * 1024.0;

#[derive(Debug, Clone, PartialEq)]
pub struct UnitSample {
    pub name: String,
    pub mem_bytes: Option<u64>,
    /// Total CPU time since the unit started. `None` while it isn't running.
    pub cpu_ns: Option<u64>,
}

/// Parses the output of `systemctl show -p Id,MemoryCurrent,CPUUsageNSec`
pub fn parse_show(output: &str) -> Result<Vec<UnitSample>, ParseError> {
    output
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(parse_block)
        .collect()
}

fn parse_block(block: &str) -> Result<UnitSample, ParseError> {
    let mut name = None;
    let mut mem_bytes = None;
    let mut cpu_ns = None;

    for line in block.lines() {
        match line.split_once('=') {
            Some(("Id", id)) => name = Some(id.strip_suffix(".service").unwrap_or(id)),
            Some(("MemoryCurrent", value)) => mem_bytes = value.parse().ok(),
            Some(("CPUUsageNSec", value)) => cpu_ns = value.parse().ok(),
            _ => {}
        }
    }

    let name = name.ok_or(ParseError("systemctl show"))?;
    Ok(UnitSample {
        name: name.to_string(),
        mem_bytes,
        cpu_ns,
    })
}

/// Each unit's figures from two samples `elapsed` apart
pub fn service_metrics(
    earlier: &[UnitSample],
    later: &[UnitSample],
    elapsed: Duration,
    cores: usize,
) -> Vec<ServiceMetrics> {
    later
        .iter()
        .map(|unit| {
            let before = earlier.iter().find(|sample| sample.name == unit.name);
            ServiceMetrics {
                name: unit.name.clone(),
                mem_mb: unit.mem_bytes.map(|bytes| bytes as f64 / MIB),
                cpu_pct: cpu_pct(before.and_then(|b| b.cpu_ns), unit.cpu_ns, elapsed, cores),
            }
        })
        .collect()
}

/// CPU as a share of the whole node
fn cpu_pct(
    before: Option<u64>,
    after: Option<u64>,
    elapsed: Duration,
    cores: usize,
) -> Option<f64> {
    let used_ns = after?.checked_sub(before?)?;
    let capacity_ns = elapsed.as_nanos() as f64 * cores as f64;
    if capacity_ns == 0.0 {
        return None;
    }
    Some(used_ns as f64 / capacity_ns * 100.0)
}

pub fn read(units: &[String]) -> Result<Vec<UnitSample>, ReadError> {
    if units.is_empty() {
        return Ok(Vec::new());
    }
    let output = Command::new("systemctl")
        .args(["show", "-p", "Id,MemoryCurrent,CPUUsageNSec"])
        .args(units)
        .output()
        .map_err(|source| ReadError::Io {
            path: "systemctl",
            source,
        })?;

    if !output.status.success() {
        return Err(ReadError::Command {
            command: "systemctl show",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(parse_show(&String::from_utf8_lossy(&output.stdout))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW_1: &str = include_str!("../fixtures/systemctl-show-1.txt");
    const SHOW_2: &str = include_str!("../fixtures/systemctl-show-2.txt");
    const FIVE_SECONDS: Duration = Duration::from_secs(5);

    fn find<'a>(metrics: &'a [ServiceMetrics], name: &str) -> &'a ServiceMetrics {
        metrics.iter().find(|m| m.name == name).unwrap()
    }

    #[test]
    fn parses_every_unit() {
        let units = parse_show(SHOW_1).unwrap();
        let names: Vec<_> = units.iter().map(|unit| unit.name.as_str()).collect();

        assert_eq!(
            names,
            [
                "mosquitto",
                "poller-tfl",
                "poller-weather",
                "data-service",
                "node-metrics"
            ]
        );
        assert_eq!(units[0].mem_bytes, Some(6111232));
        assert_eq!(units[0].cpu_ns, Some(51094000));
    }

    #[test]
    fn a_unit_that_is_not_running_has_no_figures() {
        let units = parse_show(SHOW_1).unwrap();
        let node_metrics = &units[4];

        assert_eq!(node_metrics.mem_bytes, None);
        assert_eq!(node_metrics.cpu_ns, None);
    }

    #[test]
    fn rejects_a_block_without_an_id() {
        assert!(parse_show("MemoryCurrent=1\nCPUUsageNSec=2\n").is_err());
    }

    #[test]
    fn calculates_figures_between_samples() {
        let earlier = parse_show(SHOW_1).unwrap();
        let later = parse_show(SHOW_2).unwrap();
        let metrics = service_metrics(&earlier, &later, FIVE_SECONDS, 4);

        // 336,000 ns of CPU out of 4 cores × 5 s.
        let mosquitto = find(&metrics, "mosquitto");
        let cpu = mosquitto.cpu_pct.unwrap();
        assert!((cpu - 0.00168).abs() < 0.00001, "{cpu}");
        // 5,062,656 bytes.
        let mem = mosquitto.mem_mb.unwrap();
        assert!((mem - 4.83).abs() < 0.01, "{mem}");

        assert_eq!(find(&metrics, "poller-tfl").cpu_pct, Some(0.0));
        assert_eq!(find(&metrics, "node-metrics").cpu_pct, None);
    }

    #[test]
    fn the_first_sample_has_no_cpu_figures() {
        let later = parse_show(SHOW_2).unwrap();
        let metrics = service_metrics(&[], &later, FIVE_SECONDS, 4);

        assert!(metrics.iter().all(|m| m.cpu_pct.is_none()));
        assert!(find(&metrics, "mosquitto").mem_mb.is_some());
    }

    #[test]
    fn a_restart_between_samples_gives_no_cpu_figure() {
        let unit = |cpu_ns| UnitSample {
            name: "poller-tfl".to_string(),
            mem_bytes: Some(1),
            cpu_ns: Some(cpu_ns),
        };
        let metrics = service_metrics(&[unit(900)], &[unit(100)], FIVE_SECONDS, 4);

        assert_eq!(metrics[0].cpu_pct, None);
    }
}
