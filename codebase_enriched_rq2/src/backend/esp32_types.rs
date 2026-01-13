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
    /// Creates SetVariable command to update ESP32 variable value.
    /// Sets variable identified by name to given u32 value. Used for runtime parameter configuration on ESP32.
    pub fn set_variable(name: String, value: u32) -> Self {
        Self::SetVariable { name, value }
    }

    /// Creates StartOption command to execute named function on ESP32.
    /// Triggers ESP32 to execute function/option identified by option string. Used for remote function calls.
    pub fn start_option(option: String) -> Self {
        Self::StartOption { start_option: option }
    }

    /// Creates Reset command to reboot ESP32 device.
    /// Sends reset signal to ESP32, causes device reboot and TCP connection drop. Returns Reset{reset: true} command.
    pub fn reset() -> Self {
        Self::Reset { reset: true }
    }

    /// Creates GetStatus command to request ESP32 device status.
    /// Queries ESP32 for current status information (variables, uptime, etc). Returns GetStatus command.
    pub fn get_status() -> Self {
        Self::GetStatus
    }

    /// Serializes command to JSON string for TCP transmission to ESP32.
    /// Converts Esp32Command enum variant to JSON with appropriate structure (setVariable, startOption, reset, getStatus).
    /// Returns JSON string or serde_json error if serialization fails.
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
    /// Creates ConnectionStatus event indicating ESP32 connection state change.
    /// Constructs event with connected flag, device IP address, and TCP/UDP port numbers. Used to notify WebSocket clients of connection changes.
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
    /// Creates ESP32 device configuration for TCP connection with IP address and ports.
    /// Initializes config with device_id, IP, TCP/UDP ports, sets auto_connect to false, device_source to TCP, UDP timeout to 10 seconds.
    /// Used for standard ESP32 devices with TCP/UDP connectivity.
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

    /// Creates ESP32 device configuration for UART serial connection.
    /// Initializes config with device_id, dummy IP 0.0.0.0, ports 0, device_source UART, 30-second timeout.
    /// Used for ESP32 devices connected via serial/UART instead of network.
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

    /// Creates ESP32 device configuration for UDP-only connection identified by MAC address.
    /// Initializes config with MAC address as both device_id and device_name, IP and UDP port, tcp_port 0,
    /// device_source UDP with MAC, 30-second timeout. Used for ESP32 devices discovered via mDNS/UDP without TCP.
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


    /// Returns TCP socket address (IP:port) for TCP connection.
    /// Combines ip_address and tcp_port into SocketAddr. Used for establishing TCP connections to ESP32.
    pub fn tcp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_address, self.tcp_port)
    }

    /// Returns UDP socket address (IP:port) for UDP communication.
    /// Combines ip_address and udp_port into SocketAddr. Used for UDP broadcast listening from ESP32.
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
    /// Checks if connection state is Connected.
    /// Returns true if state is ConnectionState::Connected, false otherwise. Used to verify active ESP32 connection.
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }

    /// Checks if connection state is Connecting.
    /// Returns true if state is ConnectionState::Connecting, false otherwise. Used to detect connection in progress.
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