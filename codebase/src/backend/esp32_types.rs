// ESP32 communication types and protocol definitions

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

// ============================================================================
// ESP32 COMMAND TYPES - Messages sent to ESP32
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Esp32Command {
    /// Set a variable value on the ESP32
    SetVariable {
        name: String,
        value: u32,
    },
    /// Send a start option/function to execute
    StartOption {
        #[serde(rename = "startOption")]
        start_option: String,
    },
    /// Send reset command to ESP32
    Reset {
        reset: bool,
    },
    /// Request current status/info from ESP32
    GetStatus,
}

impl Esp32Command {
    pub fn set_variable(name: String, value: u32) -> Self {
        Self::SetVariable { name, value }
    }
    
    pub fn start_option(option: String) -> Self {
        Self::StartOption { start_option: option }
    }
    
    pub fn reset() -> Self {
        Self::Reset { reset: true }
    }
    
    pub fn get_status() -> Self {
        Self::GetStatus
    }
    
    /// Serialize command to JSON for TCP transmission
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            Self::SetVariable { name, value } => {
                let cmd = serde_json::json!({
                    "setVariable": {
                        "name": name,
                        "value": value
                    }
                });
                serde_json::to_string(&cmd)
            }
            Self::StartOption { start_option } => {
                let cmd = serde_json::json!({
                    "startOption": start_option
                });
                serde_json::to_string(&cmd)
            }
            Self::Reset { reset } => {
                let cmd = serde_json::json!({
                    "reset": reset
                });
                serde_json::to_string(&cmd)
            }
            Self::GetStatus => {
                let cmd = serde_json::json!({
                    "getStatus": true
                });
                serde_json::to_string(&cmd)
            }
        }
    }
}

// ============================================================================
// ESP32 EVENT TYPES - Messages received from ESP32
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Esp32Event {
    /// Variable update from ESP32
    VariableUpdate {
        name: String,
        value: String,
    },
    /// Available start options from ESP32
    StartOptions {
        #[serde(rename = "startOptions")]
        options: Vec<String>,
    },
    /// Available changeable variables from ESP32
    ChangeableVariables {
        #[serde(rename = "changeableVariables")]
        variables: Vec<Esp32Variable>,
    },
    /// Raw UDP broadcast message
    UdpBroadcast {
        message: String,
        from_ip: String,
        from_port: u16,
    },
    /// TCP connection status change
    ConnectionStatus {
        connected: bool,
        device_ip: String,
        tcp_port: u16,
        udp_port: u16,
    },
    /// ESP32 device information
    DeviceInfo {
        device_id: String,
        device_name: Option<String>,
        firmware_version: Option<String>,
        uptime: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Esp32Variable {
    pub name: String,
    pub value: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

impl Esp32Event {
    pub fn connection_status(connected: bool, device_ip: IpAddr, tcp_port: u16, udp_port: u16) -> Self {
        Self::ConnectionStatus {
            connected,
            device_ip: device_ip.to_string(),
            tcp_port,
            udp_port,
        }
    }
}

// ============================================================================
// ESP32 DEVICE CONFIGURATION
// ============================================================================

/// Device source type - indicates how the device is connected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceSource {
    /// Device connected via UDP (identified by MAC address)
    Udp { mac_address: String },
    /// Device connected via UART (identified by device_id in messages)
    Uart,
    /// Device connected via TCP (identified by IP address)
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Esp32DeviceConfig {
    pub device_id: String,
    pub device_name: String,
    pub ip_address: IpAddr,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub auto_connect: bool,
    pub auto_start_option: Option<String>,
    pub udp_timeout_seconds: u64,
    /// Device source (UDP with MAC, UART, or TCP)
    pub device_source: DeviceSource,
}

impl Esp32DeviceConfig {
    pub fn new(device_id: String, ip_address: IpAddr, tcp_port: u16, udp_port: u16) -> Self {
        Self {
            device_name: device_id.clone(),
            device_id,
            ip_address,
            tcp_port,
            udp_port,
            auto_connect: false,
            auto_start_option: None,
            udp_timeout_seconds: 10, // Default: 10 seconds UDP timeout
            device_source: DeviceSource::Tcp, // Default to TCP for backward compatibility
        }
    }

    /// Create UART device config (IP is dummy 0.0.0.0)
    pub fn new_uart(device_id: String) -> Self {
        Self {
            device_name: device_id.clone(),
            device_id,
            ip_address: "0.0.0.0".parse().unwrap(),
            tcp_port: 0,
            udp_port: 0,
            auto_connect: false,
            auto_start_option: None,
            udp_timeout_seconds: 30, // Default: 30 seconds timeout for UART
            device_source: DeviceSource::Uart,
        }
    }

    /// Create UDP device config (MAC address is the device_id)
    pub fn new_udp(mac_address: String, ip_address: IpAddr, udp_port: u16) -> Self {
        Self {
            device_name: mac_address.clone(),
            device_id: mac_address.clone(), // MAC address IS the device_id
            ip_address,
            tcp_port: 0, // UDP devices don't use TCP
            udp_port,
            auto_connect: false,
            auto_start_option: None,
            udp_timeout_seconds: 30, // Default: 30 seconds UDP timeout
            device_source: DeviceSource::Udp { mac_address }, // MAC also stored in DeviceSource
        }
    }
    
    pub fn tcp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_address, self.tcp_port)
    }
    
    pub fn udp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_address, self.udp_port)
    }
}

// ============================================================================
// CONNECTION STATUS TRACKING
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }
    
    pub fn is_connecting(&self) -> bool {
        matches!(self, ConnectionState::Connecting)
    }
}

// ============================================================================
// ERROR TYPES
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum Esp32Error {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("TCP error: {0}")]
    TcpError(#[from] std::io::Error),
    
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Communication timeout")]
    Timeout,
}

pub type Esp32Result<T> = Result<T, Esp32Error>;