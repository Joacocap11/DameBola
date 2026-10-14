//! Fase 2: comunicación P2P en LAN. Ver docs/ARCHITECTURE.md y docs/NETWORK_PROTOCOL.md.

pub mod antispam;
pub mod discovery;
pub mod identity;
pub mod ipfilter;
pub mod protocol;
pub mod service;
pub mod store;
pub mod tls;

#[cfg(test)]
mod tests;
