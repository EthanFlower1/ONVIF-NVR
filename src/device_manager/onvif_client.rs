// onvif_camera.rs
// Drop this file into your project and import the OnvifCamera struct

use chrono::{NaiveDate, Utc};
use onvif::soap::{self, client::AuthType};
use schema::{self, onvif::Capabilities, transport};
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt;
use tracing::{debug, warn};
use url::Url;

// Custom error type for OnvifCamera operations
#[derive(Debug, Clone)]
pub struct OnvifError(pub String);

impl fmt::Display for OnvifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ONVIF error: {}", self.0)
    }
}

impl std::error::Error for OnvifError {}

// Implement From<String> for OnvifError
impl From<String> for OnvifError {
    fn from(err: String) -> Self {
        OnvifError(err)
    }
}

// Implement From<transport::Error> for OnvifError
impl From<transport::Error> for OnvifError {
    fn from(err: transport::Error) -> Self {
        OnvifError(err.to_string())
    }
}

// For your error.rs file - add this to make OnvifError integrate with your main error system
// This allows your ApiError conversion to work automatically
// -------
// impl From<OnvifError> for Error {
//     fn from(err: OnvifError) -> Self {
//         Error::Onvif(err.0)
//     }
// }
// -------

pub struct OnvifCamera {
    devicemgmt: soap::client::Client,
    event: Option<soap::client::Client>,
    deviceio: Option<soap::client::Client>,
    media: Option<soap::client::Client>,
    media2: Option<soap::client::Client>,
    imaging: Option<soap::client::Client>,
    ptz: Option<soap::client::Client>,
    analytics: Option<soap::client::Client>,
}

#[derive(Debug)]
pub struct StreamUri {
    pub token: String,
    pub name: String,
    pub uri: String,
    pub video_encoding: Option<String>,
    pub video_resolution: Option<(u32, u32)>,
    pub framerate: Option<f64>,
    pub bitrate: Option<u32>,
    pub audio_encoding: Option<String>,
    pub audio_bitrate: Option<u32>,
    pub audio_samplerate: Option<u32>,
}

#[derive(Debug)]
pub struct SnapshotUri {
    pub token: String,
    pub name: String,
    pub uri: String,
}

/// Builder for OnvifCamera configuration
pub struct OnvifCameraBuilder {
    uri: Option<Url>,
    service_path: String,
    username: Option<String>,
    password: Option<String>,
    fix_time: bool,
    auth_type: AuthType,
}

impl OnvifCameraBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self {
            uri: None,
            service_path: "onvif/device_service".to_string(),
            username: None,
            password: None,
            fix_time: false,
            auth_type: AuthType::Any,
        }
    }

    /// Set the camera's base URI (e.g., "http://192.168.1.100")
    pub fn uri(mut self, uri: &str) -> Result<Self, OnvifError> {
        self.uri = Some(Url::parse(uri).map_err(|e| OnvifError(e.to_string()))?);
        Ok(self)
    }

    /// Set the service path (default: "onvif/device_service")
    pub fn service_path(mut self, path: &str) -> Self {
        self.service_path = path.to_string();
        self
    }

    /// Set the username and password for authentication
    pub fn credentials(mut self, username: &str, password: &str) -> Self {
        self.username = Some(username.to_string());
        self.password = Some(password.to_string());
        self
    }

    /// Set whether to fix time differences between camera and client
    pub fn fix_time(mut self, fix: bool) -> Self {
        self.fix_time = fix;
        self
    }

    /// Set the authentication type: "any", "digest", or "usernametoken"
    pub fn auth_type(mut self, auth_type: &str) -> Self {
        self.auth_type = match auth_type.to_ascii_lowercase().as_str() {
            "digest" => AuthType::Digest,
            "usernametoken" => AuthType::UsernameToken,
            _ => AuthType::Any,
        };
        self
    }

    /// Build the OnvifCamera client
    pub async fn build(self) -> Result<OnvifCamera, OnvifError> {
        let creds = match (self.username.as_ref(), self.password.as_ref()) {
            (Some(username), Some(password)) => Some(soap::client::Credentials {
                username: username.clone(),
                password: password.clone(),
            }),
            (None, None) => None,
            _ => {
                return Err(OnvifError(
                    "Username and password must be specified together".to_string(),
                ))
            }
        };

        let base_uri = self
            .uri
            .as_ref()
            .ok_or_else(|| OnvifError("URI must be specified.".to_string()))?;

        let devicemgmt_uri = base_uri
            .join(&self.service_path)
            .map_err(|e| OnvifError(e.to_string()))?;

        let devicemgmt = soap::client::ClientBuilder::new(&devicemgmt_uri)
            .credentials(creds.clone())
            .auth_type(self.auth_type.clone())
            .build();

        let mut camera = OnvifCamera {
            devicemgmt,
            imaging: None,
            ptz: None,
            event: None,
            deviceio: None,
            media: None,
            media2: None,
            analytics: None,
        };

        let time_gap = if self.fix_time {
            let device_time = schema::devicemgmt::get_system_date_and_time(
                &camera.devicemgmt,
                &Default::default(),
            )
            .await
            .map_err(|e| OnvifError(e.to_string()))?
            .system_date_and_time;

            if let Some(utc_time) = &device_time.utc_date_time {
                let pc_time = Utc::now();
                let date = &utc_time.date;
                let t = &utc_time.time;
                let device_time =
                    NaiveDate::from_ymd_opt(date.year, date.month as _, date.day as _)
                        .unwrap()
                        .and_hms_opt(t.hour as _, t.minute as _, t.second as _)
                        .unwrap()
                        .and_utc();

                let diff = device_time - pc_time;
                if diff.num_seconds().abs() > 60 {
                    camera.devicemgmt.set_fix_time_gap(Some(diff));
                }
                Some(diff)
            } else {
                warn!("GetSystemDateAndTimeResponse doesn't have utc_data_time value!");
                None
            }
        } else {
            None
        };

        // Discover available services
        let services = schema::devicemgmt::get_services(&camera.devicemgmt, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        for service in &services.service {
            let service_url = Url::parse(&service.x_addr).map_err(|e| OnvifError(e.to_string()))?;

            if !service_url.as_str().starts_with(base_uri.as_str()) {
                return Err(OnvifError(format!(
                    "Service URI {} is not within base URI {}",
                    service_url, base_uri
                )));
            }

            let svc = Some(
                soap::client::ClientBuilder::new(&service_url)
                    .credentials(creds.clone())
                    .auth_type(self.auth_type.clone())
                    .fix_time_gap(time_gap)
                    .build(),
            );

            match service.namespace.as_str() {
                "http://www.onvif.org/ver10/device/wsdl" => {
                    if service_url != devicemgmt_uri {
                        return Err(OnvifError(format!(
                            "advertised device mgmt uri {} not expected {}",
                            service_url, devicemgmt_uri
                        )));
                    }
                }
                "http://www.onvif.org/ver10/events/wsdl" => camera.event = svc,
                "http://www.onvif.org/ver10/deviceIO/wsdl" => camera.deviceio = svc,
                "http://www.onvif.org/ver10/media/wsdl" => camera.media = svc,
                "http://www.onvif.org/ver20/media/wsdl" => camera.media2 = svc,
                "http://www.onvif.org/ver20/imaging/wsdl" => camera.imaging = svc,
                "http://www.onvif.org/ver20/ptz/wsdl" => camera.ptz = svc,
                "http://www.onvif.org/ver20/analytics/wsdl" => camera.analytics = svc,
                _ => debug!("unknown service: {:?}", service),
            }
        }

        Ok(camera)
    }
}

impl OnvifCamera {
    /// Get device capabilities
    pub async fn get_capabilities(&self) -> Result<Capabilities, OnvifError> {
        match schema::devicemgmt::get_capabilities(&self.devicemgmt, &Default::default()).await {
            Ok(response) => Ok(response.capabilities),
            Err(e) => Err(OnvifError(e.to_string())),
        }
    }

    /// Get device information (model, manufacturer, firmware, etc.)
    pub async fn get_device_information(
        &self,
    ) -> Result<schema::devicemgmt::GetDeviceInformationResponse, OnvifError> {
        schema::devicemgmt::get_device_information(&self.devicemgmt, &Default::default())
            .await
            .map_err(|e| e.into())
    }

    /// Get service capabilities for all available services
    pub async fn get_service_capabilities(
        &self,
    ) -> HashMap<String, Result<schema::event::GetServiceCapabilitiesResponse, OnvifError>> {
        let mut results = HashMap::new();

        // Try to get capabilities for each service
        match schema::event::get_service_capabilities(&self.devicemgmt, &Default::default()).await {
            Ok(capability) => results.insert("devicemgmt".to_string(), Ok(capability)),
            Err(error) => {
                results.insert("devicemgmt".to_string(), Err(OnvifError(error.to_string())))
            }
        };

        if let Some(ref event) = self.event {
            match schema::event::get_service_capabilities(event, &Default::default()).await {
                Ok(capability) => results.insert("event".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("event".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        if let Some(ref deviceio) = self.deviceio {
            match schema::event::get_service_capabilities(deviceio, &Default::default()).await {
                Ok(capability) => results.insert("deviceio".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("deviceio".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        if let Some(ref media) = self.media {
            match schema::event::get_service_capabilities(media, &Default::default()).await {
                Ok(capability) => results.insert("media".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("media".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        if let Some(ref media2) = self.media2 {
            match schema::event::get_service_capabilities(media2, &Default::default()).await {
                Ok(capability) => results.insert("media2".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("media2".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        if let Some(ref imaging) = self.imaging {
            match schema::event::get_service_capabilities(imaging, &Default::default()).await {
                Ok(capability) => results.insert("imaging".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("imaging".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        if let Some(ref ptz) = self.ptz {
            match schema::event::get_service_capabilities(ptz, &Default::default()).await {
                Ok(capability) => results.insert("ptz".to_string(), Ok(capability)),
                Err(error) => results.insert("ptz".to_string(), Err(OnvifError(error.to_string()))),
            };
        }

        if let Some(ref analytics) = self.analytics {
            match schema::event::get_service_capabilities(analytics, &Default::default()).await {
                Ok(capability) => results.insert("analytics".to_string(), Ok(capability)),
                Err(error) => {
                    results.insert("analytics".to_string(), Err(OnvifError(error.to_string())))
                }
            };
        }

        results
    }

    /// Get camera system date and time
    pub async fn get_system_date_and_time(
        &self,
    ) -> Result<schema::devicemgmt::GetSystemDateAndTimeResponse, OnvifError> {
        schema::devicemgmt::get_system_date_and_time(&self.devicemgmt, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))
    }

    /// Get RTSP stream URIs for all profiles
    pub async fn get_stream_uris(&self) -> Result<Vec<StreamUri>, OnvifError> {
        let media_client = self
            .media
            .as_ref()
            .ok_or_else(|| OnvifError("Client media is not available".into()))?;

        let profiles = schema::media::get_profiles(media_client, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        debug!("get_profiles response: {:#?}", &profiles);

        let requests: Vec<_> = profiles
            .profiles
            .iter()
            .map(|p: &schema::onvif::Profile| schema::media::GetStreamUri {
                profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
                stream_setup: schema::onvif::StreamSetup {
                    stream: schema::onvif::StreamType::RtpUnicast,
                    transport: schema::onvif::Transport {
                        protocol: schema::onvif::TransportProtocol::Rtsp,
                        tunnel: vec![],
                    },
                },
            })
            .collect();

        let responses = futures_util::future::try_join_all(
            requests
                .iter()
                .map(|r| schema::media::get_stream_uri(media_client, r)),
        )
        .await
        .map_err(|e| OnvifError(e.to_string()))?;

        let mut result = Vec::new();
        for (p, resp) in profiles.profiles.iter().zip(responses.iter()) {
            let mut stream_uri = StreamUri {
                token: p.token.0.clone(),
                name: p.name.0.clone(),
                uri: resp.media_uri.uri.clone(),
                video_encoding: None,
                video_resolution: None,
                framerate: None,
                bitrate: None,
                audio_encoding: None,
                audio_bitrate: None,
                audio_samplerate: None,
            };

            if let Some(ref v) = p.video_encoder_configuration {
                stream_uri.video_encoding = Some(format!("{:?}", v.encoding));
                stream_uri.video_resolution = Some((
                    v.resolution.width.try_into().unwrap_or(0),
                    v.resolution.height.try_into().unwrap_or(0),
                ));

                if let Some(ref r) = v.rate_control {
                    stream_uri.framerate = Some(r.frame_rate_limit as f64);
                    stream_uri.bitrate = Some(r.bitrate_limit.try_into().unwrap_or(0));
                }
            }

            if let Some(ref a) = p.audio_encoder_configuration {
                stream_uri.audio_encoding = Some(format!("{:?}", a.encoding));
                stream_uri.audio_bitrate = Some(a.bitrate.try_into().unwrap_or(0));
                stream_uri.audio_samplerate = Some(a.sample_rate.try_into().unwrap_or(0));
            }

            result.push(stream_uri);
        }

        Ok(result)
    }

    /// Get JPEG snapshot URIs for all profiles
    pub async fn get_snapshot_uris(&self) -> Result<Vec<SnapshotUri>, OnvifError> {
        let media_client = self
            .media
            .as_ref()
            .ok_or_else(|| OnvifError("Client media is not available".into()))?;

        let profiles = schema::media::get_profiles(media_client, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        debug!("get_profiles response: {:#?}", &profiles);

        let requests: Vec<_> = profiles
            .profiles
            .iter()
            .map(|p: &schema::onvif::Profile| schema::media::GetSnapshotUri {
                profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
            })
            .collect();

        let responses = futures_util::future::try_join_all(
            requests
                .iter()
                .map(|r| schema::media::get_snapshot_uri(media_client, r)),
        )
        .await
        .map_err(|e| OnvifError(e.to_string()))?;

        let mut result = Vec::new();
        for (p, resp) in profiles.profiles.iter().zip(responses.iter()) {
            let snapshot_uri = SnapshotUri {
                token: p.token.0.clone(),
                name: p.name.0.clone(),
                uri: resp.media_uri.uri.clone(),
            };

            result.push(snapshot_uri);
        }

        Ok(result)
    }

    /// Get camera hostname
    pub async fn get_hostname(&self) -> Result<String, OnvifError> {
        let resp = schema::devicemgmt::get_hostname(&self.devicemgmt, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        debug!("get_hostname response: {:#?}", &resp);

        Ok(resp.hostname_information.name.unwrap_or_default())
    }

    /// Set camera hostname
    pub async fn set_hostname(&self, hostname: String) -> Result<(), OnvifError> {
        schema::devicemgmt::set_hostname(
            &self.devicemgmt,
            &schema::devicemgmt::SetHostname { name: hostname },
        )
        .await
        .map_err(|e| OnvifError(e.to_string()))?;

        Ok(())
    }

    /// Enable analytics on all profiles
    pub async fn enable_analytics(&self) -> Result<(), OnvifError> {
        let media_client = self
            .media
            .as_ref()
            .ok_or_else(|| OnvifError("Client media is not available".into()))?;

        let mut config =
            schema::media::get_metadata_configurations(media_client, &Default::default())
                .await
                .map_err(|e| OnvifError(e.to_string()))?;

        if config.configurations.len() != 1 {
            return Err(OnvifError("Expected exactly one analytics config".into()));
        }

        let mut c = config.configurations.pop().unwrap();
        let token_str = c.token.0.clone();
        debug!("Metadata configuration: {:#?}", &c);

        if c.analytics != Some(true) || c.events.is_none() {
            debug!(
                "Enabling analytics in metadata configuration {}",
                &token_str
            );
            c.analytics = Some(true);
            c.events = Some(schema::onvif::EventSubscription {
                filter: None,
                subscription_policy: None,
            });

            schema::media::set_metadata_configuration(
                media_client,
                &schema::media::SetMetadataConfiguration {
                    configuration: c,
                    force_persistence: true,
                },
            )
            .await
            .map_err(|e| OnvifError(e.to_string()))?;
        } else {
            debug!(
                "Analytics already enabled in metadata configuration {}",
                &token_str
            );
        }

        let profiles = schema::media::get_profiles(media_client, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        let requests: Vec<_> = profiles
            .profiles
            .iter()
            .filter_map(
                |p: &schema::onvif::Profile| match p.metadata_configuration {
                    Some(_) => None,
                    None => Some(schema::media::AddMetadataConfiguration {
                        profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
                        configuration_token: schema::onvif::ReferenceToken(token_str.clone()),
                    }),
                },
            )
            .collect();

        if !requests.is_empty() {
            debug!(
                "Enabling metadata on {}/{} configs",
                requests.len(),
                profiles.profiles.len()
            );

            futures_util::future::try_join_all(
                requests
                    .iter()
                    .map(|r| schema::media::add_metadata_configuration(media_client, r)),
            )
            .await
            .map_err(|e| OnvifError(e.to_string()))?;
        } else {
            debug!(
                "Metadata already enabled on {} configs",
                profiles.profiles.len()
            );
        }

        Ok(())
    }

    /// Get analytics configurations
    pub async fn get_analytics(
        &self,
    ) -> Result<schema::media::GetVideoAnalyticsConfigurationsResponse, OnvifError> {
        let media_client = self
            .media
            .as_ref()
            .ok_or_else(|| OnvifError("Client media is not available".into()))?;

        let config =
            schema::media::get_video_analytics_configurations(media_client, &Default::default())
                .await
                .map_err(|e| OnvifError(e.to_string()))?;

        Ok(config)
    }

    /// Get supported analytics modules for a specific configuration
    pub async fn get_supported_analytics_modules(
        &self,
        config_token: &str,
    ) -> Result<schema::analytics::GetSupportedAnalyticsModulesResponse, OnvifError> {
        let analytics_client = self
            .analytics
            .as_ref()
            .ok_or_else(|| OnvifError("Client analytics is not available".into()))?;

        let mods = schema::analytics::get_supported_analytics_modules(
            analytics_client,
            &schema::analytics::GetSupportedAnalyticsModules {
                configuration_token: schema::onvif::ReferenceToken(config_token.to_string()),
            },
        )
        .await
        .map_err(|e| OnvifError(e.to_string()))?;

        Ok(mods)
    }

    /// Get PTZ status for the primary media profile
    pub async fn get_ptz_status(&self) -> Result<schema::ptz::GetStatusResponse, OnvifError> {
        let ptz_client = self
            .ptz
            .as_ref()
            .ok_or_else(|| OnvifError("Client PTZ is not available".into()))?;

        let media_client = self
            .media
            .as_ref()
            .ok_or_else(|| OnvifError("Client media is not available".into()))?;

        let profile = &schema::media::get_profiles(media_client, &Default::default())
            .await
            .map_err(|e| OnvifError(e.to_string()))?
            .profiles[0];

        let profile_token = schema::onvif::ReferenceToken(profile.token.0.clone());
        let status = schema::ptz::get_status(ptz_client, &schema::ptz::GetStatus { profile_token })
            .await
            .map_err(|e| OnvifError(e.to_string()))?;

        Ok(status)
    }

    /// Fetches all available information from the camera
    pub async fn get_all(&self) -> HashMap<String, Result<String, String>> {
        let mut results = HashMap::new();

        // Get system date and time
        match self.get_system_date_and_time().await {
            Ok(time) => results.insert(
                "system_date_and_time".to_string(),
                Ok(format!("{:#?}", time)),
            ),
            Err(e) => results.insert("system_date_and_time".to_string(), Err(e.to_string())),
        };

        // Get capabilities
        match self.get_capabilities().await {
            Ok(caps) => results.insert("capabilities".to_string(), Ok(format!("{:#?}", caps))),
            Err(e) => results.insert("capabilities".to_string(), Err(e.to_string())),
        };

        // Get service capabilities
        let service_caps = self.get_service_capabilities().await;
        for (key, value) in service_caps {
            match value {
                Ok(caps) => results.insert(
                    format!("service_capabilities_{}", key),
                    Ok(format!("{:#?}", caps)),
                ),
                Err(e) => {
                    results.insert(format!("service_capabilities_{}", key), Err(e.to_string()))
                }
            };
        }

        // Get device information
        match self.get_device_information().await {
            Ok(info) => {
                results.insert("device_information".to_string(), Ok(format!("{:#?}", info)))
            }
            Err(e) => results.insert("device_information".to_string(), Err(e.to_string())),
        };

        // Get stream URIs
        match self.get_stream_uris().await {
            Ok(uris) => results.insert("stream_uris".to_string(), Ok(format!("{:#?}", uris))),
            Err(e) => results.insert("stream_uris".to_string(), Err(e.to_string())),
        };

        // Get snapshot URIs
        match self.get_snapshot_uris().await {
            Ok(uris) => results.insert("snapshot_uris".to_string(), Ok(format!("{:#?}", uris))),
            Err(e) => results.insert("snapshot_uris".to_string(), Err(e.to_string())),
        };

        // Get hostname
        match self.get_hostname().await {
            Ok(hostname) => results.insert("hostname".to_string(), Ok(hostname)),
            Err(e) => results.insert("hostname".to_string(), Err(e.to_string())),
        };

        // Get analytics
        match self.get_analytics().await {
            Ok(analytics) => {
                results.insert("analytics".to_string(), Ok(format!("{:#?}", analytics)))
            }
            Err(e) => results.insert("analytics".to_string(), Err(e.to_string())),
        };

        // Get PTZ status
        match self.get_ptz_status().await {
            Ok(status) => results.insert("ptz_status".to_string(), Ok(format!("{:#?}", status))),
            Err(e) => results.insert("ptz_status".to_string(), Err(e.to_string())),
        };

        results
    }
}
