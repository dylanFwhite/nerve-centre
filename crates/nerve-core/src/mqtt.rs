//! A service's connection to the broker: publishing envelopes and,
//! optionally, receiving messages.

use std::time::Duration;

use chrono::{DateTime, Utc};
use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Outgoing, Packet, Publish, QoS,
};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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

/// A message received from the broker. Acknowledge it with
/// [`Connection::ack`] once it has been handled.
#[derive(Debug)]
pub struct Message(Publish);

impl Message {
    pub fn topic(&self) -> &str {
        &self.0.topic
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload
    }
}

/// What a subscribing service listens to, and where messages go.
struct Subscription {
    filter: String,
    sender: mpsc::Sender<Message>,
}

/// A service's connection to the broker.
pub struct Connection {
    client: AsyncClient,
    topics: Topics,
    service: String,
    event_loop_task: JoinHandle<()>,
}

impl Connection {
    /// Connects for publishing only
    pub fn connect(config: &MqttConfig, service: &str) -> Result<Self, MqttError> {
        Self::start(config, service, None)
    }

    /// Connects with a persistent session subscribed to `filter`.
    pub fn subscribe(
        config: &MqttConfig,
        service: &str,
        filter: String,
    ) -> Result<(Self, mpsc::Receiver<Message>), MqttError> {
        let (sender, receiver) = mpsc::channel(100);
        let mqtt = Self::start(config, service, Some(Subscription { filter, sender }))?;
        Ok((mqtt, receiver))
    }

    fn start(
        config: &MqttConfig,
        service: &str,
        subscription: Option<Subscription>,
    ) -> Result<Self, MqttError> {
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

        if subscription.is_some() {
            // Keep the session, and any unacknowledged messages, across
            // restarts, and acknowledge only when the service declares
            options.set_clean_session(false);
            options.set_manual_acks(true);
        }

        let (client, mut event_loop) = AsyncClient::new(options, 10);
        let event_loop_client = client.clone();

        // The event loop does the actual network work and must be polled
        // continuously. After an error, the next poll reconnects.
        let event_loop_task = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        eprintln!("mqtt: connected");
                        // Subscribe on every connect, in case the broker has
                        // expired the session.
                        if let Some(subscription) = &subscription
                            && let Err(error) = event_loop_client
                                .try_subscribe(&subscription.filter, QoS::AtLeastOnce)
                        {
                            eprintln!("mqtt: can't subscribe: {error}")
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        if let Some(subscription) = &subscription {
                            let _ = subscription.sender.send(Message(publish)).await;
                        }
                    }
                    Ok(Event::Outgoing(Outgoing::Disconnect))
                    | Err(ConnectionError::RequestsDone) => {
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("mqtt: {error}; retrying in 5 s");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Self {
            client,
            topics,
            service: service.to_string(),
            event_loop_task,
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

    pub async fn ack(&self, message: &Message) -> Result<(), MqttError> {
        self.client.ack(&message.0).await?;
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

    pub async fn disconnect(self, last_success: Option<DateTime<Utc>>) -> Result<(), MqttError> {
        let status = ServiceStatus {
            state: ServiceState::Offline,
            last_success,
        };
        self.publish(&self.topics.service(&self.service), status)
            .await?;
        self.client.disconnect().await?;

        let _ = tokio::time::timeout(Duration::from_secs(5), self.event_loop_task).await;
        Ok(())
    }
}
