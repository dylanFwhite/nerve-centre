//! Topic names. Every topic starts with a root from config:
//! `home` on the Pi, `dev` on the dev machine.

use std::fmt;

#[derive(Debug, Clone)]
pub struct Topics {
    root: String,
}

impl Topics {
    /// Accepts a root such as `home` or `dev`. A trailing `/` is removed.
    pub fn new(root: &str) -> Result<Self, InvalidRoot> {
        let root = root.trim_end_matches('/');
        if root.is_empty() || root.contains(['+', '#']) {
            return Err(InvalidRoot(root.to_string()));
        }
        Ok(Self {
            root: root.to_string(),
        })
    }

    /// Current status of every line.
    pub fn tfl_status(&self) -> String {
        format!("{}/tfl/status", self.root)
    }

    /// Planned disruptions for the next 7 days.
    pub fn tfl_planned(&self) -> String {
        format!("{}/tfl/planned", self.root)
    }

    /// Weather now and for the rest of today.
    pub fn weather_forecast(&self) -> String {
        format!("{}/weather/forecast", self.root)
    }

    /// Health of one service, e.g. `service("poller-tfl")`.
    pub fn service(&self, name: &str) -> String {
        format!("{}/service/{name}", self.root)
    }

    /// Subscription filter matching the health of every service.
    pub fn all_services(&self) -> String {
        format!("{}/service/+", self.root)
    }

    /// Subscription filter matching every topic under the root.
    pub fn all(&self) -> String {
        format!("{}/#", self.root)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvalidRoot(pub String);

impl fmt::Display for InvalidRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid topic root {:?}: must be non-empty and contain no '+' or '#'",
            self.0
        )
    }
}

impl std::error::Error for InvalidRoot {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_topics_under_the_root() {
        let topics = Topics::new("home").unwrap();
        assert_eq!(topics.tfl_status(), "home/tfl/status");
        assert_eq!(topics.tfl_planned(), "home/tfl/planned");
        assert_eq!(topics.weather_forecast(), "home/weather/forecast");
        assert_eq!(topics.service("poller-tfl"), "home/service/poller-tfl");
        assert_eq!(topics.all_services(), "home/service/+");
        assert_eq!(topics.all(), "home/#");
    }

    #[test]
    fn removes_a_trailing_slash() {
        let topics = Topics::new("dev/").unwrap();
        assert_eq!(topics.tfl_status(), "dev/tfl/status");
    }

    #[test]
    fn rejects_empty_and_wildcard_roots() {
        assert!(Topics::new("").is_err());
        assert!(Topics::new("/").is_err());
        assert!(Topics::new("home/#").is_err());
        assert!(Topics::new("+").is_err());
    }
}
