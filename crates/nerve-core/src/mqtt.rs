//! Connecting to the broker and publishing envelopes.

use std::time::Duration;

use chrono::{DateTime, Utc};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use thiserror::Error;

use crate::config::MqttConfig;
use crate::envelope::{Envelope, Payload};
use crate::payloads::service::{ServiceState, ServiceStatus};
use crate::topics::{InvalidRoot, Topics};

#[derive(Debug, Error)]
pub enum MqttError {
    #[error(transparent)]
    InvalidRoot(#[from] InvalidRoot),
    #[error("can't encode message: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("MQTT client stopped: {0}")]
    Client(#[from] rumqttc::ClientError),
}

/// A service's connection to the broker, for publishing.
#[derive(Clone)]
pub struct Publisher {
    client: AsyncClient,
    topics: Topics,
    service: String,
}

impl Publisher {
    pub fn connect(config: &MqttConfig, service: &str) -> Result<Self, MqttError> {
        let topics = Topics::new(&config.topic_root)?;

        let client_id = format!("{}-{service}", config.topic_root);

        let offline = ServiceStatus {
            state: ServiceState::Offline,
            last_success: None,
        };
        let last_will = LastWill::new(
            topics.service(service),
            Envelope::new(service, offline).to_json()?,
            QoS::AtLeastOnce,
            true,
        );

        let mut options = MqttOptions::new(client_id, &config.host, config.port);
        options.set_credentials(&config.username, &config.password);
        options.set_keep_alive(Duration::from_secs(30));
        options.set_last_will(last_will);

        let (client, mut event_loop) = AsyncClient::new(options, 10);

        // The event loop does the actual network work and must be polled
        // continuously. After an error, the next poll reconnects.
        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        eprintln!("mqtt: connected");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprint!("mqtt: {error}; retrying in 5 s");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Self {
            client,
            topics,
            service: service.to_string(),
        })
    }

    pub fn topics(&self) -> &Topics {
        &self.topics
    }

    pub async fn publish<T: Payload>(&self, topic: &str, data: T) -> Result<(), MqttError> {
        let json = Envelope::new(&self.service, data).to_json()?;
        self.client
            .publish(topic, QoS::AtLeastOnce, true, json)
            .await?;
        Ok(())
    }

    pub async fn report_online(
        &self,
        last_success: Option<DateTime<Utc>>,
    ) -> Result<(), MqttError> {
        let status = ServiceStatus {
            state: ServiceState::Online,
            last_success,
        };
        self.publish(&self.topics.service(&self.service), status)
            .await
    }
}
