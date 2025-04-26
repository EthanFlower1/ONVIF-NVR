use anyhow::Result;
use log::{error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;
// Use local crate paths instead of g_streamer
use crate::config::MessageBrokerConfig;
use crate::messaging::broker::create_message_broker;
use crate::messaging::EventType;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();
    
    info!("Starting RabbitMQ integration test");

    // Create a message broker config
    let config = MessageBrokerConfig {
        uri: "amqp://guest:guest@localhost:5672/%2f".to_string(),
        retry_attempts: 3,
        ..MessageBrokerConfig::default()
    };

    info!("Connecting to RabbitMQ at {}", config.uri);

    // Create a message broker
    let broker = match create_message_broker(config.clone()).await {
        Ok(broker) => {
            info!("✅ Successfully connected to RabbitMQ");
            broker
        }
        Err(e) => {
            error!("❌ Failed to connect to RabbitMQ: {}", e);
            error!("Make sure RabbitMQ is running at {}", config.uri);
            error!("You can install RabbitMQ using Docker: docker run -d --name rabbitmq -p 5672:5672 -p 15672:15672 rabbitmq:3-management");
            return Err(e);
        }
    };

    // Flag to track if we received messages (shared between threads)
    let message_received = Arc::new(AtomicBool::new(false));
    let message_received_clone = message_received.clone();

    // Generate unique test IDs
    let test_id = Uuid::new_v4();
    let camera_id = Uuid::new_v4();

    // Subscribe to all events
    info!("Setting up subscription for test events");
    let subscription = broker
        .subscribe_pattern("#", Arc::new(move |event| {
            info!("📩 Received event: {} (ID: {})", event.event_type, event.id);
            info!("   Payload: {}", serde_json::to_string_pretty(&event.payload).unwrap_or_default());
            
            // Check if this is our test event
            if let Some(payload_test_id) = event.payload.get("test_id") {
                if let Some(id_str) = payload_test_id.as_str() {
                    if id_str == test_id.to_string() {
                        message_received_clone.store(true, Ordering::SeqCst);
                        info!("✅ Successfully received our test event!");
                    }
                }
            }
            
            Ok(())
        }))
        .await?;
    
    info!("✅ Subscribed to events with ID: {}", subscription);

    // Allow the subscription to be set up
    sleep(Duration::from_secs(1)).await;

    // Test 1: System event (no source ID)
    info!("Test 1: Publishing system event");
    broker
        .publish(
            EventType::SystemStartup,
            None,
            serde_json::json!({
                "test_id": test_id.to_string(),
                "message": "System test event",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
        .await?;

    // Sleep to allow message processing
    sleep(Duration::from_secs(2)).await;

    // Test 2: Camera-specific event (with source ID)
    info!("Test 2: Publishing camera event");
    broker
        .publish(
            EventType::CameraConnected,
            Some(camera_id),
            serde_json::json!({
                "test_id": test_id.to_string(),
                "camera_name": "Test Camera",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
        .await?;

    // Wait for events to be processed
    info!("Waiting for events to be processed...");
    sleep(Duration::from_secs(3)).await;

    // Check if we received any messages
    if message_received.load(Ordering::SeqCst) {
        info!("✅ Successfully verified message publishing and subscribing!");
    } else {
        warn!("⚠️ No test messages were received. There might be an issue with message routing.");
    }

    // Unsubscribe
    broker.unsubscribe(&subscription).await?;
    info!("✅ Unsubscribed from events");

    // Wait for things to clean up
    sleep(Duration::from_secs(1)).await;
    
    info!("🎉 RabbitMQ integration test completed");
    info!("Run this test again with different event types to verify full functionality");

    Ok(())
}