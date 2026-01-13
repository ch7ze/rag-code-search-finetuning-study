use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use tracing::{info, warn};
use tokio::sync::mpsc;

/// mDNS server for advertising the ESP32 Manager Server
pub struct MdnsServer {
    daemon: Option<ServiceDaemon>,
    service_info: Option<ServiceInfo>,
    stop_tx: Option<mpsc::UnboundedSender<()>>,
    is_running: bool,
}

impl MdnsServer {
    /// Creates new mDNS server instance for advertising ESP32 Manager Server on local network.
    /// Initializes empty MdnsServer with no daemon, service_info, or stop channel. Call start_advertising() to activate.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            daemon: None,
            service_info: None,
            stop_tx: None,
            is_running: false,
        })
    }

    /// Starts mDNS advertising for ESP32 Manager Server with HTTP service announcement.
    /// Creates ServiceDaemon, gets local IP address, registers "_http._tcp.local." service named "esp-server.local."
    /// with TXT records (version, path, type, protocol). Spawns keep-alive task. Returns error if already running or daemon creation fails.
    pub async fn start_advertising(&mut self, port: u16) -> Result<(), String> {
        if self.is_running {
            return Err("mDNS server already running".to_string());
        }

        // Create mDNS daemon
        let daemon = ServiceDaemon::new()
            .map_err(|e| format!("Failed to create mDNS daemon: {}", e))?;

        // Get local IP addresses
        let local_ips = self.get_local_ip_addresses()?;
        if local_ips.is_empty() {
            return Err("No local IP addresses found".to_string());
        }

        // Create TXT records with server information
        let mut properties = HashMap::new();
        properties.insert("version".to_string(), "1.0".to_string());
        properties.insert("path".to_string(), "/".to_string());
        properties.insert("type".to_string(), "esp32-manager".to_string());
        properties.insert("protocol".to_string(), "http".to_string());

        // Create service info for HTTP service
        let service_info = ServiceInfo::new(
            "_http._tcp.local.",
            "esp-server",
            "esp-server.local.",
            local_ips[0],
            port,
            properties,
        ).map_err(|e| format!("Failed to create service info: {}", e))?;

        // Register the service
        daemon.register(service_info.clone())
            .map_err(|e| format!("Failed to register mDNS service: {}", e))?;

        info!("mDNS server advertising 'esp-server.local' on port {} with IPs: {:?}", port, local_ips);

        self.daemon = Some(daemon);
        self.service_info = Some(service_info);
        self.is_running = true;

        // Start keep-alive task
        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel();
        self.stop_tx = Some(stop_tx);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => {
                        info!("Stopping mDNS server advertising");
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                        // Keep daemon alive by doing nothing - the service stays registered
                    }
                }
            }
        });

        Ok(())
    }

    /// Stops mDNS advertising and unregisters service from local network.
    /// Sends stop signal to keep-alive task, unregisters service from daemon, shuts down daemon. Sets is_running to false.
    pub async fn stop_advertising(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }

        if let (Some(daemon), Some(service_info)) = (self.daemon.take(), self.service_info.take()) {
            if let Err(e) = daemon.unregister(service_info.get_fullname()) {
                warn!("Failed to unregister mDNS service: {}", e);
            }

            if let Err(e) = daemon.shutdown() {
                warn!("Failed to shutdown mDNS daemon: {}", e);
            }
        }

        self.is_running = false;
        info!("mDNS server advertising stopped");
    }

    /// Gets local IP address of server by connecting to external address.
    /// Creates UDP socket, connects to 8.8.8.8:80, extracts local_addr from socket. Returns Vec with single IP address.
    /// Used to determine server IP for mDNS service registration.
    fn get_local_ip_addresses(&self) -> Result<Vec<IpAddr>, String> {
        use std::net::UdpSocket;

        // Try to get the local IP by connecting to a remote address
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("Failed to create socket: {}", e))?;

        socket.connect("8.8.8.8:80")
            .map_err(|e| format!("Failed to connect to remote: {}", e))?;

        let local_addr = socket.local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?;

        Ok(vec![local_addr.ip()])
    }

    /// Returns true if mDNS server is actively advertising, false otherwise.
    /// Checks is_running flag. Used to verify mDNS advertisement status.
    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

impl Drop for MdnsServer {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }

        if let (Some(daemon), Some(service_info)) = (self.daemon.take(), self.service_info.take()) {
            let _ = daemon.unregister(service_info.get_fullname());
            let _ = daemon.shutdown();
        }
    }
}