use crate::config::MessageBrokerConfig;
use crate::error::Error;
use crate::messaging::event::{EventMessage, EventType};
use anyhow::Result;
use async_trait::async_trait;
use deadpool_lapin::{Config, Manager, Pool, PoolError};
use futures_util::stream::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, Consumer, ExchangeKind,
};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Callback function type for event handling
pub type EventCallback = Arc<dyn Fn(EventMessage) -> Result<()> + Send + Sync>;

/// Message broker service trait
#[async_trait]
pub trait MessageBrokerTrait: Send + Sync {
    /// Publish an event
    async fn publish<T: Serialize + Send>(&self, event_type: EventType, source_id: Option<Uuid>, payload: T) -> Result<()>;
    
    /// Subscribe to an event type
    async fn subscribe(&self, event_type: EventType, callback: EventCallback) -> Result<String>;
    
    /// Subscribe to all events from a specific source
    async fn subscribe_source(&self, source_id: Uuid, callback: EventCallback) -> Result<String>;
    
    /// Subscribe to a specific routing pattern
    async fn subscribe_pattern(&self, pattern: &str, callback: EventCallback) -> Result<String>;
    
    /// Unsubscribe from a subscription
    async fn unsubscribe(&self, subscription_id: &str) -> Result<()>;
}

/// RabbitMQ message broker implementation
pub struct MessageBroker {
    /// Connection pool
    pool: Pool,
    /// Configuration
    config: MessageBrokerConfig,
    /// Subscriptions map
    subscriptions: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
    /// Default channel
    channel: Arc<Mutex<Option<Channel>>>,
}

impl MessageBroker {
    /// Create a new message broker
    pub async fn new(config: MessageBrokerConfig) -> Result<Self> {
        // Create RabbitMQ connection pool
        // Create pool config using the deadpool-lapin API
        let pool_config = Config {
            url: Some(config.uri.clone()),
            pool: Some(deadpool_lapin::PoolConfig {
                max_size: config.pool_size as usize,
                queue_mode: deadpool::managed::QueueMode::Fifo, // Use First-In-First-Out mode
                timeouts: deadpool::managed::Timeouts {
                    wait: Some(Duration::from_millis(config.timeout_ms)),
                    create: Some(Duration::from_millis(config.timeout_ms)),
                    recycle: Some(Duration::from_millis(config.timeout_ms)),
                }
            }),
            connection_properties: ConnectionProperties::default(),
        };
        let pool = pool_config.create_pool(Some(deadpool_lapin::Runtime::Tokio1))?;

        // Create the broker
        let broker = Self {
            pool,
            config,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            channel: Arc::new(Mutex::new(None)),
        };

        // Initialize broker (create exchanges)
        broker.init().await?;

        Ok(broker)
    }

    /// Initialize the message broker (create exchanges)
    async fn init(&self) -> Result<()> {
        // Get a connection from the pool
        let conn = self.get_amqp_connection().await?;
        
        // Create a channel
        let channel = conn.create_channel().await
            .map_err(|e| Error::Service(format!("Failed to create RabbitMQ channel: {}", e)))?;
        
        // Declare the main exchange
        channel
            .exchange_declare(
                &self.config.exchange,
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    auto_delete: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to declare exchange: {}", e)))?;
        
        // Declare the dead letter exchange
        channel
            .exchange_declare(
                &self.config.dead_letter_exchange,
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    auto_delete: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to declare DLX exchange: {}", e)))?;
        
        // Store default channel
        let mut default_channel = self.channel.lock().await;
        *default_channel = Some(channel);
        
        info!("RabbitMQ message broker initialized");
        
        Ok(())
    }
    
    /// Get a connection from the pool with retry
    async fn get_connection(&self) -> Result<deadpool::managed::Object<Manager>> {
        let mut attempts = 0;
        let max_attempts = self.config.retry_attempts;
        
        loop {
            attempts += 1;
            match self.pool.get().await {
                Ok(conn) => return Ok(conn),
                Err(err) => {
                    if attempts >= max_attempts {
                        return Err(Error::Service(format!("Failed to get RabbitMQ connection after {} attempts: {}", 
                            attempts, err)).into());
                    }
                    
                    warn!("Failed to get RabbitMQ connection (attempt {}/{}): {}", 
                         attempts, max_attempts, err);
                         
                    // Wait before retry
                    tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                }
            }
        }
    }
    
    /// Get the AMQP connection from a pool object
    async fn get_amqp_connection(&self) -> Result<Connection> {
        // Get a connection from the pool
        let _conn = self.get_connection().await?;
        // Get the AMQP connection from the object
        // Need to create a new connection since we can't easily get the inner connection
        let amqp_conn = Connection::connect(
            &self.config.uri,
            ConnectionProperties::default(),
        ).await.map_err(|e| Error::Service(format!("Failed to create AMQP connection: {}", e)))?;
        
        Ok(amqp_conn)
    }
    
    /// Get the default channel or create a new one
    async fn get_channel(&self) -> Result<Channel> {
        let mut channel_guard = self.channel.lock().await;
        
        if let Some(channel) = &*channel_guard {
            if channel.status().connected() {
                return Ok(channel.clone());
            }
        }
        
        // If we get here, we need a new channel
        let conn = self.get_amqp_connection().await?;
        let channel = conn.create_channel().await
            .map_err(|e| Error::Service(format!("Failed to create RabbitMQ channel: {}", e)))?;
            
        *channel_guard = Some(channel.clone());
        
        Ok(channel)
    }
    
    /// Create a consumer queue for the given routing pattern
    async fn create_consumer_queue(&self, pattern: &str) -> Result<(Channel, String, Consumer)> {
        // Get a channel
        let channel = self.get_channel().await?;
        
        // Create a queue with a unique name
        let queue_name = format!("gstreamer.{}.{}", pattern.replace(".", "_"), Uuid::new_v4());
        
        // Declare arguments for dead letter exchange
        let mut args = FieldTable::default();
        args.insert("x-dead-letter-exchange".into(), lapin::types::AMQPValue::LongString(self.config.dead_letter_exchange.clone().into()));
        
        // Declare the queue
        let _queue = channel
            .queue_declare(
                &queue_name,
                QueueDeclareOptions {
                    exclusive: true,
                    auto_delete: true,
                    ..Default::default()
                },
                args,
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to declare queue: {}", e)))?;
            
        debug!("Created queue: {} for pattern: {}", queue_name, pattern);
        
        // Bind queue to exchange
        channel
            .queue_bind(
                &queue_name,
                &self.config.exchange,
                pattern,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to bind queue: {}", e)))?;
            
        // Create consumer
        let consumer = channel
            .basic_consume(
                &queue_name,
                &format!("consumer-{}", Uuid::new_v4()),
                BasicConsumeOptions {
                    no_ack: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to create consumer: {}", e)))?;
            
        Ok((channel, queue_name, consumer))
    }
    
    /// Start a consumer process for the given routing pattern and callback
    async fn start_consumer(&self, pattern: &str, callback: EventCallback) -> Result<String> {
        // Create consumer queue
        let (_channel, _queue_name, mut consumer) = self.create_consumer_queue(pattern).await?;
        
        // Generate a subscription ID
        let subscription_id = Uuid::new_v4().to_string();
        
        // Clone references for the async task
        let subscription_id_clone = subscription_id.clone();
        // Store pattern as a String to avoid lifetime issues
        let pattern_owned = pattern.to_string();
        
        // Start the consumer task
        let handle = tokio::spawn(async move {
            info!("Started consumer for pattern: {} (subscription: {})", pattern_owned, subscription_id_clone);
            
            while let Some(delivery) = consumer.next().await {
                match delivery {
                    Ok(delivery) => {
                        let delivery = delivery;
                        
                        // Try to parse message
                        match serde_json::from_slice::<EventMessage>(&delivery.data) {
                            Ok(event) => {
                                debug!("Received event: {} ({})", event.event_type, event.id);
                                
                                // Call the callback
                                match callback(event) {
                                    Ok(_) => {
                                        // Acknowledge the message
                                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                            error!("Failed to acknowledge message: {}", e);
                                        }
                                    },
                                    Err(e) => {
                                        // Log the error but still acknowledge to avoid blocking
                                        error!("Error processing event: {}", e);
                                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                            error!("Failed to acknowledge message: {}", e);
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to parse event message: {}", e);
                                // Acknowledge the message anyway to avoid blocking
                                if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                    error!("Failed to acknowledge message: {}", e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Error receiving message: {}", e);
                        // Short delay to avoid tight loop on errors
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
            
            info!("Consumer stopped for pattern: {} (subscription: {})", pattern_owned, subscription_id_clone);
        });
        
        // Store the subscription
        self.subscriptions.write().await.insert(subscription_id.clone(), handle);
        
        Ok(subscription_id)
    }
}

#[async_trait]
impl MessageBrokerTrait for MessageBroker {
    async fn publish<T: Serialize + Send>(&self, event_type: EventType, source_id: Option<Uuid>, payload: T) -> Result<()> {
        // Create event message
        let event = EventMessage::new(event_type, source_id, payload)?;
        
        // Serialize the event
        let message = serde_json::to_vec(&event)?;
        
        // Get a channel
        let channel = self.get_channel().await?;
        
        // Get the routing key
        let routing_key = event.routing_key();
        
        // Publish the message
        channel
            .basic_publish(
                &self.config.exchange,
                &routing_key,
                BasicPublishOptions::default(),
                &message,
                BasicProperties::default(),
            )
            .await
            .map_err(|e| Error::Service(format!("Failed to publish message: {}", e)))?;
            
        debug!("Published event: {} with routing key: {}", event.id, routing_key);
        
        Ok(())
    }
    
    async fn subscribe(&self, event_type: EventType, callback: EventCallback) -> Result<String> {
        // Create routing pattern for event type
        let pattern = event_type.to_string();
        
        // Start consumer
        self.start_consumer(&pattern, callback).await
    }
    
    async fn subscribe_source(&self, source_id: Uuid, callback: EventCallback) -> Result<String> {
        // Create routing pattern for source ID
        let pattern = format!("*.{}", source_id);
        
        // Start consumer
        self.start_consumer(&pattern, callback).await
    }
    
    async fn subscribe_pattern(&self, pattern: &str, callback: EventCallback) -> Result<String> {
        // Start consumer with the given pattern
        self.start_consumer(pattern, callback).await
    }
    
    async fn unsubscribe(&self, subscription_id: &str) -> Result<()> {
        // Get the subscriptions map
        let mut subscriptions = self.subscriptions.write().await;
        
        // Find and remove the subscription
        if let Some(handle) = subscriptions.remove(subscription_id) {
            // Abort the task
            handle.abort();
            info!("Unsubscribed: {}", subscription_id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Subscription not found: {}", subscription_id)).into())
        }
    }
}

/// Create a message broker service
pub async fn create_message_broker(config: MessageBrokerConfig) -> Result<Arc<MessageBroker>> {
    // Create the broker
    let broker = MessageBroker::new(config).await?;
    
    Ok(Arc::new(broker))
}