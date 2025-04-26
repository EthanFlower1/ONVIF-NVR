#[cfg(test)]
mod tests {
    use super::broker::create_message_broker;
    use super::event::{EventMessage, EventType};
    use crate::config::MessageBrokerConfig;
    use anyhow::Result;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::sleep;

    // Test that we can create a message broker
    #[tokio::test]
    async fn test_create_message_broker() -> Result<()> {
        // Skip test if no RabbitMQ is available
        if std::env::var("TEST_RABBITMQ").is_err() {
            println!("Skipping RabbitMQ test. Set TEST_RABBITMQ=1 to run.");
            return Ok(());
        }
        
        let config = MessageBrokerConfig::default();
        let broker = create_message_broker(config).await?;
        
        // If we got here, we successfully connected
        assert!(true);
        Ok(())
    }
    
    // Test that we can publish and subscribe to events
    #[tokio::test]
    async fn test_publish_subscribe() -> Result<()> {
        // Skip test if no RabbitMQ is available
        if std::env::var("TEST_RABBITMQ").is_err() {
            println!("Skipping RabbitMQ test. Set TEST_RABBITMQ=1 to run.");
            return Ok(());
        }
        
        let config = MessageBrokerConfig {
            exchange: format!("test.exchange.{}", uuid::Uuid::new_v4()),
            ..MessageBrokerConfig::default()
        };
        
        let broker = create_message_broker(config.clone()).await?;
        
        // Create a variable to hold received events
        let received = Arc::new(Mutex::new(Vec::<EventMessage>::new()));
        let received_clone = received.clone();
        
        // Subscribe to events
        let _sub_id = broker.subscribe(
            EventType::SystemStartup,
            Arc::new(move |event| {
                let mut events = received_clone.lock().unwrap();
                events.push(event);
                Ok(())
            }),
        ).await?;
        
        // Wait a moment for subscription to be ready
        sleep(Duration::from_millis(500)).await;
        
        // Publish an event
        broker.publish(
            EventType::SystemStartup,
            None,
            serde_json::json!({"test": true}),
        ).await?;
        
        // Wait for event to be received
        sleep(Duration::from_millis(1000)).await;
        
        // Check that we received the event
        let events = received.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::SystemStartup);
        
        Ok(())
    }
}