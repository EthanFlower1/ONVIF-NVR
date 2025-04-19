export interface Camera {
  id: string // UUID string in TypeScript
  name: string
  model?: string // Optional fields use ? in TypeScript
  manufacturer?: string
  ip_address: string
  username?: string
  password?: string
  onvif_endpoint?: string
  status: string
  primary_stream_id?: string // UUID string
  sub_stream_id?: string // UUID string
  firmware_version?: string
  serial_number?: string
  hardware_id?: string
  mac_address?: string
  ptz_supported?: boolean
  audio_supported?: boolean
  analytics_supported?: boolean

  // Events support
  events_supported?: any // JSON value translated to any
  event_notification_endpoint?: string

  // Storage information
  has_local_storage?: boolean
  storage_type?: string
  storage_capacity_gb?: number
  storage_used_gb?: number
  retention_days?: number
  recording_mode?: string

  // Analytics information
  analytics_capabilities?: any // JSON value translated to any
  ai_processor_type?: string
  ai_processor_model?: string
  object_detection_supported?: boolean
  face_detection_supported?: boolean
  license_plate_recognition_supported?: boolean
  person_tracking_supported?: boolean
  line_crossing_supported?: boolean
  zone_intrusion_supported?: boolean
  object_classification_supported?: boolean
  behavior_analysis_supported?: boolean

  // Original fields
  capabilities?: any // JSON value translated to any
  profiles?: any // JSON value translated to any
  last_updated?: string // DateTime converted to string, can use Date in TS
  created_at: string // DateTime converted to string, can use Date in TS
  updated_at: string // DateTime converted to string, can use Date in TS
  created_by: string // UUID string
}
