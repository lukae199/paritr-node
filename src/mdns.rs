use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::{config::Config, consensus};

const SERVICE_TYPE: &str = "_paritr-node._tcp.local.";

/// Advertise the local management UI. `mdns-sd` performs mDNS name probing and
/// conflict resolution; the persisted random device suffix makes a collision
/// exceptionally unlikely even when many fresh appliances start together.
pub fn register(config: &Config) -> anyhow::Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    let port = config
        .management_bind
        .parse::<std::net::SocketAddr>()?
        .port();
    let hostname = format!("{}.local.", config.device_name);
    let properties = [
        ("id", config.device_id.as_str()),
        ("chain", consensus::CHAIN_ID),
        ("version", consensus::NODE_VERSION),
        ("path", "/"),
    ];
    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &config.device_name,
        &hostname,
        "",
        port,
        &properties[..],
    )?
    .enable_addr_auto();
    daemon.register(service)?;
    Ok(daemon)
}

/// Check whether a user-selected hostname is already advertised by another
/// Paritr node. The permanent random first-run name remains the final fallback
/// if multicast discovery is unavailable.
pub async fn name_available(name: &str, own_device_id: &str) -> anyhow::Result<bool> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(SERVICE_TYPE)?;
    let hostname = format!("{name}.local.");
    let deadline = tokio::time::Instant::now() + Duration::from_millis(900);
    let mut available = true;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, receiver.recv_async()).await {
            Ok(Ok(ServiceEvent::ServiceResolved(service)))
                if service.get_hostname().eq_ignore_ascii_case(&hostname)
                    && service.get_property_val_str("id") != Some(own_device_id) =>
            {
                available = false;
                break;
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => break,
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(available)
}
