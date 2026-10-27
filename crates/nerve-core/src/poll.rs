//! Helpers for services that poll a source on a schedule and publish
//! what they find.

use std::time::Duration;

use tokio::time::{Interval, MissedTickBehavior, interval};

use crate::envelope::Payload;
use crate::mqtt::{Connection, MqttError};

pub fn ticker(seconds: u64) -> Interval {
    let mut ticker = interval(Duration::from_secs(seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

pub struct PublishOnChange<T> {
    topic: String,
    last: Option<T>,
}

impl<T: Payload + Clone> PublishOnChange<T> {
    pub fn new(topic: String) -> Self {
        Self { topic, last: None }
    }

    /// Publishes `data` if it has changed. The first call always publishes.
    pub async fn publish(&mut self, mqtt: &Connection, data: T) -> Result<(), MqttError> {
        if self.last.as_ref() != Some(&data) {
            mqtt.publish(&self.topic, data.clone()).await?;
            println!("published {}", self.topic);
            self.last = Some(data);
        }
        Ok(())
    }
}
