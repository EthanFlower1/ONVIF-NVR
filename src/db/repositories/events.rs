use crate::error::Error;
use crate::models::Event;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Events repository for handling event operations
#[derive(Clone)]
pub struct EventsRepository {
    pool: Arc<PgPool>,
}

impl EventsRepository {
    /// Create a new events repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
    
    /// Create a new event
    pub async fn create(&self, event: &Event) -> Result<Event> {
        let result = sqlx::query_as::<_, Event>(
            r#"
            INSERT INTO events (
                id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            "#
        )
        .bind(event.id)
        .bind(event.camera_id)
        .bind(&event.event_type)
        .bind(&event.topic)
        .bind(event.timestamp)
        .bind(&event.source_name)
        .bind(&event.source_value)
        .bind(&event.data)
        .bind(event.recording_id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create event: {}", e)))?;
        
        Ok(result)
    }
    
    /// Get event by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<Event>> {
        let result = sqlx::query_as::<_, Event>(
            r#"
            SELECT id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            FROM events
            WHERE id = $1
            "#
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get event by ID: {}", e)))?;
        
        Ok(result)
    }
    
    /// Get events for a camera
    pub async fn get_by_camera(&self, camera_id: &Uuid, limit: Option<i64>) -> Result<Vec<Event>> {
        let limit = limit.unwrap_or(100);
        
        let result = sqlx::query_as::<_, Event>(
            r#"
            SELECT id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            FROM events
            WHERE camera_id = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#
        )
        .bind(camera_id)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get events for camera: {}", e)))?;
        
        Ok(result)
    }
    
    /// Get events by type
    pub async fn get_by_type(&self, event_type: &str, limit: Option<i64>) -> Result<Vec<Event>> {
        let limit = limit.unwrap_or(100);
        
        let result = sqlx::query_as::<_, Event>(
            r#"
            SELECT id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            FROM events
            WHERE event_type = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#
        )
        .bind(event_type)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get events by type: {}", e)))?;
        
        Ok(result)
    }
    
    /// Get events in a time range
    pub async fn get_by_time_range(&self, start_time: DateTime<Utc>, end_time: DateTime<Utc>, limit: Option<i64>) -> Result<Vec<Event>> {
        let limit = limit.unwrap_or(100);
        
        let result = sqlx::query_as::<_, Event>(
            r#"
            SELECT id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            FROM events
            WHERE timestamp >= $1 AND timestamp <= $2
            ORDER BY timestamp DESC
            LIMIT $3
            "#
        )
        .bind(start_time)
        .bind(end_time)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get events in time range: {}", e)))?;
        
        Ok(result)
    }
    
    /// Delete events older than a specific date
    pub async fn delete_old_events(&self, cutoff_date: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM events
            WHERE timestamp < $1
            "#
        )
        .bind(cutoff_date)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete old events: {}", e)))?;
        
        Ok(result.rows_affected())
    }
    
    /// Search events with combined filters
    pub async fn search(&self, camera_id: Option<&Uuid>, event_type: Option<&str>, 
                       start_time: Option<DateTime<Utc>>, end_time: Option<DateTime<Utc>>, 
                       limit: Option<i64>) -> Result<Vec<Event>> {
        let limit = limit.unwrap_or(100);
        
        let mut sql = String::from(
            r#"
            SELECT id, camera_id, event_type, topic, timestamp, source_name, source_value, data, recording_id
            FROM events
            WHERE 1=1
            "#
        );
        
        let mut params = vec![];
        let mut param_index = 1;
        
        if let Some(camera_id) = camera_id {
            sql.push_str(&format!(" AND camera_id = ${}", param_index));
            params.push(serde_json::to_value(camera_id)?);
            param_index += 1;
        }
        
        if let Some(event_type) = event_type {
            sql.push_str(&format!(" AND event_type = ${}", param_index));
            params.push(serde_json::to_value(event_type)?);
            param_index += 1;
        }
        
        if let Some(start_time) = start_time {
            sql.push_str(&format!(" AND timestamp >= ${}", param_index));
            params.push(serde_json::to_value(start_time)?);
            param_index += 1;
        }
        
        if let Some(end_time) = end_time {
            sql.push_str(&format!(" AND timestamp <= ${}", param_index));
            params.push(serde_json::to_value(end_time)?);
        }
        
        sql.push_str(" ORDER BY timestamp DESC");
        sql.push_str(&format!(" LIMIT {}", limit));
        
        let mut db_query = sqlx::query_as::<_, Event>(&sql);
        
        for param in params {
            db_query = db_query.bind(param);
        }
        
        let result = db_query
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to search events: {}", e)))?;
        
        Ok(result)
    }
}