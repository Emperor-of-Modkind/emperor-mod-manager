//! Mobile companion remote protocol, LAN discovery, local-network pairing,
//! and `iroh`-powered desktop host / mobile client sync & RPC.

pub mod lan_discovery;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod pairing;
pub mod protocol;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod desktop_backend;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod host;

#[cfg(any(target_os = "android", target_os = "ios", test))]
pub mod client;
