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
    /// Create new mDNS server
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            daemon: None,
            service_info: None,
            stop_tx: None,
            is_running: false,
        })
    }

    /// Start advertising the server via mDNS
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

    /// Stop mDNS advertising
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

    /// Get local IP addresses for the server
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

    /// Check if server is running
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