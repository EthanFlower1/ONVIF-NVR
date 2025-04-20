use devicemgmt;
use futures_util::stream::StreamExt;
use media;
use onvif::{discovery, soap};
use schema::onvif as onvif_schema;
use tracing::{debug, info};
use url::Url;
use uuid::Uuid;

use crate::db::models::camera_models::Camera;

// Discover ONVIF cameras on the network and gather information without authentication
pub async fn discover() -> Result<Vec<Camera>, anyhow::Error> {
    info!("Starting ONVIF camera discovery on the network");

    let mut cameras = Vec::new();
    let discovery_results = discovery::DiscoveryBuilder::default().run().await?;

    let mut discovered_cameras = Vec::new();

    // First collect all discovered addresses
    discovery_results
        .for_each(|addr| {
            discovered_cameras.push(addr);
            async {}
        })
        .await;

    info!("Found {} potential ONVIF devices", discovered_cameras.len());

    // Process each discovered device
    let results: Vec<Result<Camera, anyhow::Error>> =
        futures_util::future::join_all(discovered_cameras.into_iter().map(|addr| async move {
            let camera = process_discovered_device(addr).await;
            camera
        }))
        .await;

    // Collect valid camera results
    for result in results {
        if let Ok(camera) = result {
            cameras.push(camera);
        }
    }

    info!(
        "Successfully gathered information for {} cameras",
        cameras.len()
    );
    Ok(cameras)
}

async fn process_discovered_device(device: discovery::Device) -> Result<Camera, anyhow::Error> {
    let mut camera = Camera::default();

    // Extract IP address
    camera.ip_address = device.urls[0].host().unwrap().to_string();
    camera.name = device.name.unwrap();

    Ok(camera)
}
