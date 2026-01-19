//! Generated types from .slint-files

use mullvad_types::constraints::Constraint;
use talpid_types::net::IpVersion;

slint::include_modules!();

impl Eq for Relay {}

impl From<&mullvad_types::states::TunnelState> for ConnectionState {
    fn from(tunnel_state: &mullvad_types::states::TunnelState) -> Self {
        match tunnel_state {
            mullvad_types::states::TunnelState::Disconnected { .. } => {
                ConnectionState::Disconnected
            }
            mullvad_types::states::TunnelState::Connecting { .. } => ConnectionState::Connecting,
            mullvad_types::states::TunnelState::Connected { .. } => ConnectionState::Connected,
            mullvad_types::states::TunnelState::Disconnecting { .. } => {
                ConnectionState::Disconnecting
            }
            mullvad_types::states::TunnelState::Error { .. } => ConnectionState::Error,
        }
    }
}

impl From<Constraint<IpVersion>> for DeviceIpVersion {
    fn from(ip_version: Constraint<IpVersion>) -> Self {
        match ip_version {
            Constraint::Any => DeviceIpVersion::Auto,
            Constraint::Only(IpVersion::V4) => DeviceIpVersion::Ipv4,
            Constraint::Only(IpVersion::V6) => DeviceIpVersion::Ipv6,
        }
    }
}

impl From<DeviceIpVersion> for Constraint<IpVersion> {
    fn from(device_ip_version: DeviceIpVersion) -> Self {
        match device_ip_version {
            DeviceIpVersion::Auto => Constraint::Any,
            DeviceIpVersion::Ipv4 => Constraint::Only(IpVersion::V4),
            DeviceIpVersion::Ipv6 => Constraint::Only(IpVersion::V6),
        }
    }
}
