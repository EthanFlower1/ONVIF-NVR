use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::default::Default;
use std::str::FromStr;
use yaserde::de::from_str;
use yaserde_derive::{YaDeserialize, YaSerialize};

// Low-level XML parsing structs (these match the XML structure)
#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "tt", namespace = "tt: http://www.onvif.org/ver10/schema")]
pub struct MetadataStream {
    #[yaserde(prefix = "tt", rename = "Event")]
    pub event: Event,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
pub struct Event {
    #[yaserde(prefix = "wsnt", rename = "NotificationMessage")]
    pub notification_message: NotificationMessage,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(
    prefix = "wsnt",
    namespace = "wsnt: http://docs.oasis-open.org/wsn/b-2",
    namespace = "tns1: http://www.onvif.org/ver10/topics",
    namespace = "wsa5: http://www.w3.org/2005/08/addressing"
)]
pub struct NotificationMessage {
    #[yaserde(prefix = "wsnt", rename = "Topic")]
    pub topic: Topic,

    #[yaserde(prefix = "wsnt", rename = "ProducerReference")]
    pub producer_reference: ProducerReference,

    #[yaserde(prefix = "wsnt", rename = "Message")]
    pub message: Message,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "wsnt")]
pub struct Topic {
    #[yaserde(attribute, rename = "Dialect")]
    pub dialect: String,

    #[yaserde(text)]
    pub value: String,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(namespace = "wsa5: http://www.w3.org/2005/08/addressing")]
pub struct ProducerReference {
    #[yaserde(
        prefix = "wsa5",
        rename = "Address",
        namespace = "wsa5: http://www.w3.org/2005/08/addressing"
    )]
    pub address: String,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "wsnt")]
pub struct Message {
    #[yaserde(prefix = "tt", rename = "Message")]
    pub tt_message: TTMessage,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "tt")]
pub struct TTMessage {
    #[yaserde(attribute, rename = "PropertyOperation")]
    pub property_operation: String,

    #[yaserde(attribute, rename = "UtcTime")]
    pub utc_time: String,

    #[yaserde(prefix = "tt", rename = "Source")]
    pub source: Source,

    #[yaserde(prefix = "tt", rename = "Data")]
    pub data: Data,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "tt")]
pub struct Source {
    #[yaserde(prefix = "tt", rename = "SimpleItem")]
    pub simple_items: Vec<SimpleItem>,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "tt")]
pub struct Data {
    #[yaserde(prefix = "tt", rename = "SimpleItem")]
    pub simple_items: Vec<SimpleItem>,
}

#[derive(Debug, YaSerialize, YaDeserialize, Default)]
#[yaserde(prefix = "tt")]
pub struct SimpleItem {
    #[yaserde(attribute, rename = "Value")]
    pub value: String,

    #[yaserde(attribute, rename = "Name")]
    pub name: String,
}

// High-level API structs
// ======================

#[derive(Debug, Clone)]
pub enum EventType {
    MotionDetected,
    AudioDetected,
    TamperDetected,
    LineDetected,
    FieldDetected,
    FaceDetected,
    ObjectDetected,
    Other(String),
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let topic = s.trim();

        if topic.contains("MotionDetector") || topic.contains("Motion") {
            Ok(EventType::MotionDetected)
        } else if topic.contains("AudioDetector") || topic.contains("Audio") {
            Ok(EventType::AudioDetected)
        } else if topic.contains("TamperDetector") || topic.contains("Tamper") {
            Ok(EventType::TamperDetected)
        } else if topic.contains("Line") || topic.contains("CrossLine") {
            Ok(EventType::LineDetected)
        } else if topic.contains("Field") {
            Ok(EventType::FieldDetected)
        } else if topic.contains("Face") {
            Ok(EventType::FaceDetected)
        } else if topic.contains("Object") {
            Ok(EventType::ObjectDetected)
        } else {
            Ok(EventType::Other(topic.to_string()))
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventSource {
    pub video_source: Option<String>,
    pub analytics_config: Option<String>,
    pub rule: Option<String>,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct OnvifEvent {
    pub event_type: EventType,
    pub topic: String,
    pub source_address: String,
    pub property_operation: String,
    pub timestamp: DateTime<Utc>,
    pub source: EventSource,
    pub is_active: Option<bool>,
    pub area_index: Option<u32>,
    pub confidence: Option<f32>,
    pub data: HashMap<String, String>,
}

impl TryFrom<MetadataStream> for OnvifEvent {
    type Error = String;

    fn try_from(stream: MetadataStream) -> Result<Self, Self::Error> {
        let notification = &stream.event.notification_message;
        let topic = notification.topic.value.trim().to_string();
        let event_type = EventType::from_str(&topic)
            .map_err(|e| format!("Failed to parse event type: {}", e))?;

        let tt_message = &notification.message.tt_message;

        // Parse timestamp - now with better error handling
        let timestamp = match DateTime::parse_from_rfc3339(&tt_message.utc_time) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                // Try an alternative format if standard RFC3339 fails
                // Some ONVIF devices don't include seconds fraction
                match chrono::NaiveDateTime::parse_from_str(
                    &tt_message.utc_time,
                    "%Y-%m-%dT%H:%M:%SZ",
                ) {
                    Ok(dt) => DateTime::<Utc>::from_utc(dt, Utc),
                    Err(_) => {
                        return Err(format!(
                            "Failed to parse timestamp '{}': {}",
                            tt_message.utc_time, e
                        ))
                    }
                }
            }
        };

        // Build source info
        let mut source = EventSource {
            video_source: None,
            analytics_config: None,
            rule: None,
            extra: HashMap::new(),
        };

        for item in &tt_message.source.simple_items {
            match item.name.as_str() {
                "VideoSourceConfigurationToken" => source.video_source = Some(item.value.clone()),
                "VideoAnalyticsConfigurationToken" => {
                    source.analytics_config = Some(item.value.clone())
                }
                "Rule" => source.rule = Some(item.value.clone()),
                _ => {
                    source.extra.insert(item.name.clone(), item.value.clone());
                }
            }
        }

        // Build data map and extract common fields
        let mut data = HashMap::new();
        let mut is_active = None;
        let mut area_index = None;
        let mut confidence = None;

        for item in &tt_message.data.simple_items {
            data.insert(item.name.clone(), item.value.clone());

            match item.name.as_str() {
                "IsMotion" | "State" | "IsActive" | "isSoundDetected" => {
                    is_active = Some(item.value.to_lowercase() == "true");
                }
                "AreaIndex" | "Index" => {
                    area_index = item.value.parse::<u32>().ok();
                }
                "Confidence" => {
                    confidence = item.value.parse::<f32>().ok();
                }
                _ => {}
            }
        }

        Ok(OnvifEvent {
            event_type,
            topic,
            source_address: notification.producer_reference.address.trim().to_string(),
            property_operation: tt_message.property_operation.clone(),
            timestamp,
            source,
            is_active,
            area_index,
            confidence,
            data,
        })
    }
}

// Helper functions for parsing and processing events
// =================================================

/// Parse ONVIF event XML and return the low-level representation
pub fn parse_raw_onvif_event(xml: &str) -> Result<MetadataStream, String> {
    from_str(xml).map_err(|e| format!("Failed to parse ONVIF event: {}", e))
}

/// Parse ONVIF event XML and return the high-level representation
pub fn parse_onvif_event(xml: &str) -> Result<OnvifEvent, String> {
    let stream = parse_raw_onvif_event(xml)?;
    OnvifEvent::try_from(stream)
}

// Convenience functions for specific event types
// =============================================

/// Check if the event is a motion detection event
pub fn is_motion_event(event: &OnvifEvent) -> bool {
    matches!(event.event_type, EventType::MotionDetected)
}

/// Check if motion is currently detected (for motion events)
pub fn is_motion_active(event: &OnvifEvent) -> Option<bool> {
    if is_motion_event(event) {
        event.is_active
    } else {
        None
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_parse_motion_event() {
//         let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//         <tt:MetadataStream xmlns:tt="http://www.onvif.org/ver10/schema">
//           <tt:Event>
//             <wsnt:NotificationMessage xmlns:tns1="http://www.onvif.org/ver10/topics" xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2" xmlns:wsa5="http://www.w3.org/2005/08/addressing">
//               <wsnt:Topic Dialect="http://www.onvif.org/ver10/tev/topicExpression/ConcreteSet">
//                   tns1:RuleEngine/CellMotionDetector/Motion
//               </wsnt:Topic>
//               <wsnt:ProducerReference>
//                   <wsa5:Address> 192.168.1.105/onvif/event/alarm </wsa5:Address>
//               </wsnt:ProducerReference>
//               <wsnt:Message>
//                   <tt:Message PropertyOperation="Initialized" UtcTime="2025-4-28T23:28:42Z">
//                       <tt:Source>
//                           <tt:SimpleItem Value="video_source_config" Name="VideoSourceConfigurationToken"></tt:SimpleItem>
//                           <tt:SimpleItem Value="analy_config" Name="VideoAnalyticsConfigurationToken"></tt:SimpleItem>
//                           <tt:SimpleItem Value="MyMotionDetectorRule" Name="Rule"></tt:SimpleItem>
//                       </tt:Source>
//                       <tt:Data>
//                          <tt:SimpleItem Value="true" Name="IsMotion"></tt:SimpleItem>
//                          <tt:SimpleItem Value="0" Name="AreaIndex"></tt:SimpleItem>
//                       </tt:Data>
//                   </tt:Message>
//               </wsnt:Message>
//              </wsnt:NotificationMessage>
//           </tt:Event>
//         </tt:MetadataStream>"#;
//
//         let event = parse_onvif_event(xml).unwrap();
//
//         assert!(matches!(event.event_type, EventType::MotionDetected));
//         assert_eq!(event.source_address, "192.168.1.105/onvif/event/alarm");
//         assert_eq!(event.property_operation, "Initialized");
//         assert_eq!(
//             event.source.video_source,
//             Some("video_source_config".to_string())
//         );
//         assert_eq!(event.source.rule, Some("MyMotionDetectorRule".to_string()));
//         assert_eq!(event.is_active, Some(true));
//         assert_eq!(event.area_index, Some(0));
//
//         assert!(is_motion_event(&event));
//         assert_eq!(is_motion_active(&event), Some(true));
//     }
// }
