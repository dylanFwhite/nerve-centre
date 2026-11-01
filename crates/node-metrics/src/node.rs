//! Node-wide figures, parsed from the text of Linux's `/proc` and `/sys`
//! files. Parsing is kept apart from reading, so it's tested on any OS.

use std::io;
use std::process::Command;

use nerve_core::payloads::node::{Throttle, ThrottleFlag};
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

/// Bit positions in `vcgencmd get_throttled` value
const THROTTLE_BITS: [(u32, ThrottleFlag); 4] = [
    (0, ThrottleFlag::UnderVoltage),
    (1, ThrottleFlag::FreqCapped),
    (2, ThrottleFlag::Throttled),
    (3, ThrottleFlag::SoftTempLimit),
];

/// Parses `vcgencmd get_throttled` output, e.g. `throttled=0x50005`.
pub fn parse_throttled(output: &str) -> Result<Throttle, ParseError> {
    let bits = output
        .trim()
        .strip_prefix("throttled=0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or(ParseError("vcgencmg get_throttled"))?;

    Ok(Throttle {
        now: throttle_flags(bits),
        since_boot: throttle_flags(bits >> 16),
    })
}

fn throttle_flags(bits: u32) -> Vec<ThrottleFlag> {
    THROTTLE_BITS
        .iter()
        .filter(|(bit, _)| bits & (1 << bit) != 0)
        .map(|&(_, flag)| flag)
        .collect()
}

/// Percentage of disk space used
pub fn disk_used_pct(total: u64, free: u64, available: u64) -> f64 {
    let used = total.saturating_sub(free);
    let usable = used + available;
    if usable == 0 {
        return 0.0;
    }
    used as f64 / usable as f64 * 100.0
}

// Reading the sources - Requires a linux node

const STAT: &str = "/proc/stat";
const MEMINFO: &str = "/proc/meminfo";
const LOADAVG: &str = "/proc/loadavg";
const UPTIME: &str = "/proc/uptime";
const THERMAL: &str = "/sys/class/thermal/thermal_zone0/temp";
const DISK: &str = "/";

#[derive(Debug, Error)]
pub enum ReadError {
    #[error("can't read {path}: {source}")]
    Io {
        path: &'static str,
        source: io::Error,
    },
    #[error(transparent)]
    Parse(#[from] ParseError),
}

#[derive(Debug)]
pub struct Reading {
    pub cpu: CpuTimes,
    pub load_1m: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temp_c: Option<f64>,
    pub throttle: Option<Throttle>,
    pub disk_used_pct: f64,
    pub uptime_s: u64,
}

impl Reading {
    pub fn take() -> Result<Self, ReadError> {
        let (mem_used_mb, mem_total_mb) = parse_memory(&read_file(MEMINFO)?)?;
        Ok(Self {
            cpu: CpuTimes::parse(&read_file(STAT)?)?,
            load_1m: parse_load_1m(&read_file(LOADAVG)?)?,
            mem_used_mb,
            mem_total_mb,
            temp_c: read_file(THERMAL)
                .ok()
                .and_then(|text| parse_temp(&text).ok()),
            throttle: read_throttle(),
            disk_used_pct: read_disk_used_pct()?,
            uptime_s: parse_uptime(&read_file(UPTIME)?)?,
        })
    }
}

fn read_file(path: &'static str) -> Result<String, ReadError> {
    std::fs::read_to_string(path).map_err(|source| ReadError::Io { path, source })
}

fn read_throttle() -> Option<Throttle> {
    let output = Command::new("vcgencmd")
        .arg("get_throttled")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_throttled(&String::from_utf8_lossy(&output.stdout)).ok()
}

fn read_disk_used_pct() -> Result<f64, ReadError> {
    let stats = fs4::statvfs(DISK).map_err(|source| ReadError::Io { path: DISK, source })?;
    Ok(disk_used_pct(
        stats.total_space(),
        stats.free_space(),
        stats.available_space(),
    ))
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

    #[test]
    fn decodes_throttle_flags() {
        // Bits 0 and 2 now, and bits 16 and 18 since boot.
        let throttle = parse_throttled("throttled=0x50005\n").unwrap();

        assert_eq!(
            throttle.now,
            [ThrottleFlag::UnderVoltage, ThrottleFlag::Throttled]
        );
        assert_eq!(
            throttle.since_boot,
            [ThrottleFlag::UnderVoltage, ThrottleFlag::Throttled]
        );
    }

    #[test]
    fn all_clear_is_two_empty_lists() {
        let throttle = parse_throttled("throttled=0x0\n").unwrap();

        assert!(throttle.now.is_empty());
        assert!(throttle.since_boot.is_empty());
        assert!(parse_throttled("Can't open device file").is_err());
    }

    #[test]
    fn calculates_disk_use_as_df_does() {
        // From `df -B1 /` on the hub: 1B-blocks, Used and Available.
        let (total, used, available) = (250368110592, 4244377600, 233331195904);
        let pct = disk_used_pct(total, total - used, available);

        // 1.79%, which df rounds up to 2%.
        assert!((pct - 1.79).abs() < 0.01, "{pct}");
    }
}
