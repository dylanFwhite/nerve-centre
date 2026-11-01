//! Node-wide figures, parsed from the text of Linux's `/proc` and `/sys`
//! files. Parsing is kept apart from reading, so it's tested on any OS.

use thiserror::Error;

/// A source's text wasn't in the expected format. Names the source.
#[derive(Debug, Error)]
#[error("unexpected format in {0}")]
pub struct ParseError(pub &'static str);

/// Cumulative CPU time since boot, across all cores, in clock ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

impl CpuTimes {
    /// Cpu metrics from first line of `/proc/stat`
    pub fn parse(stat: &str) -> Result<Self, ParseError> {
        Self::try_parse(stat).ok_or(ParseError("/proc/stat"))
    }

    fn try_parse(stat: &str) -> Option<Self> {
        let mut fields = stat.lines().next()?.split_whitespace();
        if fields.next()? != "cpu" {
            return None;
        }
        let times = fields
            .take(8)
            .map(|field| field.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        let [user, nice, system, idle, iowait, irq, softirq, steal] = times[..] else {
            return None;
        };

        let waiting = idle + iowait;
        let total = user + nice + system + waiting + irq + softirq + steal;
        Some(Self {
            busy: total - waiting,
            total,
        })
    }

    /// Percentage of time the CPU was busy since `earlier`
    pub fn busy_pct_since(&self, earlier: &CpuTimes) -> f64 {
        let total = self.total.saturating_sub(earlier.total);
        if total == 0 {
            return 0.0;
        }
        let busy = self.busy.saturating_sub(earlier.busy);
        busy as f64 / total as f64 * 100.0
    }
}

/// Memory used and total in MiB, from `proc/meminfo`.
pub fn parse_memory(meminfo: &str) -> Result<(u64, u64), ParseError> {
    let total = meminfo_kib(meminfo, "MemTotal:");
    let available = meminfo_kib(meminfo, "MemAvailable:");
    match (total, available) {
        (Some(total), Some(available)) => {
            Ok((total.saturating_sub(available) / 1024, total / 1024))
        }
        _ => Err(ParseError("/proc/meminfo")),
    }
}

/// 1-minute load average, the first number in `/proc/loadavg`
pub fn parse_load_1m(loadavg: &str) -> Result<f64, ParseError> {
    first_number(loadavg).ok_or(ParseError("/proc/loadavg"))
}

/// Whole seconds since boot, the first number in `/proc/uptime`
pub fn parse_uptime(uptime: &str) -> Result<u64, ParseError> {
    let seconds: f64 = first_number(uptime).ok_or(ParseError("/proc/uptime"))?;
    Ok(seconds as u64)
}

/// Degrees Celsius, from a thermal zone's `temp` file in thousandths
pub fn parse_temp(temp: &str) -> Result<f64, ParseError> {
    let millidegrees: f64 = first_number(temp).ok_or(ParseError("thermal zone temp"))?;
    Ok(millidegrees / 1000.0)
}

fn meminfo_kib(meminfo: &str, key: &str) -> Option<u64> {
    let line = meminfo.lines().find_map(|line| line.strip_prefix(key))?;
    line.trim().strip_suffix(" kB")?.parse().ok()
}

fn first_number(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_1: &str = include_str!("../fixtures/stat-1.txt");
    const STAT_2: &str = include_str!("../fixtures/stat-2.txt");

    #[test]
    fn parses_cpu_times() {
        // cpu  12178 3123 23394 360538452 80988 0 852 0 0 0
        let times = CpuTimes::parse(STAT_1).unwrap();

        assert_eq!(times.total, 12178 + 3123 + 23394 + 360538452 + 80988 + 852);
        assert_eq!(times.busy, 12178 + 3123 + 23394 + 852);
    }

    #[test]
    fn calculates_cpu_use_between_samples() {
        let earlier = CpuTimes::parse(STAT_1).unwrap();
        let later = CpuTimes::parse(STAT_2).unwrap();

        // 1 busy tick out of 2001 over the 5 seconds.
        let pct = later.busy_pct_since(&earlier);
        assert!((pct - 0.05).abs() < 0.001, "{pct}");
    }

    #[test]
    fn no_time_passed_means_no_cpu_use() {
        let times = CpuTimes::parse(STAT_1).unwrap();
        assert_eq!(times.busy_pct_since(&times), 0.0);
    }

    #[test]
    fn rejects_a_malformed_stat() {
        assert!(CpuTimes::parse("").is_err());
        assert!(CpuTimes::parse("intr 1 2 3").is_err());
        assert!(CpuTimes::parse("cpu  1 2 3").is_err());
    }

    #[test]
    fn parses_memory() {
        let meminfo = include_str!("../fixtures/meminfo.txt");
        // MemTotal 4146240 kB, MemAvailable 3858624 kB.
        assert_eq!(parse_memory(meminfo).unwrap(), (280, 4049));
        assert!(parse_memory("MemTotal: 100 kB").is_err());
    }

    #[test]
    fn parses_load_uptime_and_temperature() {
        let load = parse_load_1m(include_str!("../fixtures/loadavg.txt")).unwrap();
        let uptime = parse_uptime(include_str!("../fixtures/uptime.txt")).unwrap();
        let temp = parse_temp(include_str!("../fixtures/thermal-temp.txt")).unwrap();

        assert_eq!(load, 1.0);
        assert_eq!(uptime, 901865);
        assert_eq!(temp, 47.4);
    }
}
