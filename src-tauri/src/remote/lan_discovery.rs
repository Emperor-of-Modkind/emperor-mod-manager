#![allow(dead_code)]
//! Lightweight UDP LAN discovery for local-network pairing.
//!
//! Desktop responds to `EMPEROR_DISCOVER_V1` UDP probes while the pairing modal is active
//! (and optionally for fast LAN reconnection). Mobile scans the local subnet via multicast
//! and broadcast, with manual IP fallback when routers enforce AP isolation.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket as StdUdpSocket};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use super::protocol::DiscoveredDesktop;

pub const DISCOVERY_PORT: u16 = 41729;
pub const DISCOVERY_MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 69);
pub const DISCOVERY_MAGIC: &str = "EMPEROR_DISCOVER_V1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanProbe {
    pub magic: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanAnnounce {
    pub magic: String,
    pub nonce: String,
    pub host_name: String,
    pub desktop_endpoint_id: String,
    pub lan_addrs: Vec<SocketAddr>,
    pub pairing_active: bool,
    pub app_version: String,
}

/// Best-effort detection of the primary local LAN IPv4 address without external crates.
pub fn primary_lan_ip() -> Option<IpAddr> {
    let sock = StdUdpSocket::bind("0.0.0.0:0").ok()?;
    // Does not send any packets; asks the OS routing table which local interface would route out.
    sock.connect("192.0.2.1:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_unspecified() {
        None
    } else {
        Some(ip)
    }
}

/// Best-effort hostname for displaying in the pairing UI.
pub fn local_host_name() -> String {
    if let Ok(name) = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    #[cfg(unix)]
    if let Ok(raw) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "Emperor Desktop".to_string()
}

/// Mobile-side LAN scanner: sends UDP multicast + broadcast probes and collects responding desktops.
pub async fn discover_lan_desktops(timeout: Duration) -> Result<Vec<DiscoveredDesktop>> {
    let sock = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
    let _ = sock.set_broadcast(true);

    let nonce = uuid::Uuid::new_v4().to_string();
    let probe = LanProbe {
        magic: DISCOVERY_MAGIC.to_string(),
        nonce: nonce.clone(),
    };
    let payload = serde_json::to_vec(&probe)?;

    let multicast_target = SocketAddrV4::new(DISCOVERY_MULTICAST_V4, DISCOVERY_PORT);
    let broadcast_target = SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_PORT);

    // Send twice (0ms and 250ms) to handle transient Wi-Fi packet loss.
    let _ = sock.send_to(&payload, multicast_target).await;
    let _ = sock.send_to(&payload, broadcast_target).await;

    let mut found: HashMap<String, DiscoveredDesktop> = HashMap::new();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut second_sent = false;
    let mut buf = vec![0u8; 4096];

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        let wait = if !second_sent {
            remaining.min(Duration::from_millis(250))
        } else {
            remaining
        };

        match tokio::time::timeout(wait, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, src))) => {
                if let Ok(ann) = serde_json::from_slice::<LanAnnounce>(&buf[..n]) {
                    if ann.magic == DISCOVERY_MAGIC && ann.nonce == nonce {
                        let mut addrs = ann.lan_addrs;
                        // Ensure the packet source IP is also represented if port info exists.
                        if addrs.is_empty() {
                            addrs.push(src);
                        }
                        found.insert(
                            ann.desktop_endpoint_id.clone(),
                            DiscoveredDesktop {
                                host_name: ann.host_name,
                                desktop_endpoint_id: ann.desktop_endpoint_id,
                                lan_addrs: addrs,
                                pairing_active: ann.pairing_active,
                                app_version: ann.app_version,
                            },
                        );
                    }
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {
                if !second_sent {
                    second_sent = true;
                    let _ = sock.send_to(&payload, multicast_target).await;
                    let _ = sock.send_to(&payload, broadcast_target).await;
                } else {
                    break;
                }
            }
        }
    }

    let mut list: Vec<DiscoveredDesktop> = found.into_values().collect();
    list.sort_by(|a, b| {
        b.pairing_active
            .cmp(&a.pairing_active)
            .then_with(|| a.host_name.cmp(&b.host_name))
    });
    Ok(list)
}

/// Parse a `LanProbe` from raw UDP bytes, returning its nonce if valid.
pub fn parse_probe(bytes: &[u8]) -> Option<String> {
    let probe: LanProbe = serde_json::from_slice(bytes).ok()?;
    if probe.magic == DISCOVERY_MAGIC && !probe.nonce.is_empty() && probe.nonce.len() <= 128 {
        Some(probe.nonce)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_roundtrip_and_rejection() {
        let probe = LanProbe {
            magic: DISCOVERY_MAGIC.to_string(),
            nonce: "abc-123".to_string(),
        };
        let bytes = serde_json::to_vec(&probe).unwrap();
        assert_eq!(parse_probe(&bytes).as_deref(), Some("abc-123"));

        assert_eq!(parse_probe(b"not json"), None);
        let bad = LanProbe {
            magic: "WRONG".to_string(),
            nonce: "abc".to_string(),
        };
        assert_eq!(parse_probe(&serde_json::to_vec(&bad).unwrap()), None);
    }
}
