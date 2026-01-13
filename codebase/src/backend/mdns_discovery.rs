use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, trace, error};
use mdns_sd::{ServiceDaemon, ServiceEvent};

/// Discovered ESP32 device information from mDNS
#[derive(Debug, Clone)]
pub struct MdnsEsp32Device {
    pub hostname: String,
    pub ip_addresses: Vec<IpAddr>,
    pub port: u16,
    pub txt_records: HashMap<String, String>,
    pub service_name: String,
}

/// mDNS-based ESP32 discovery service
pub struct MdnsDiscovery {
    /// mDNS daemon for service discovery
    mdns_daemon: Option<ServiceDaemon>,
    /// Discovered devices cache
    discovered_devices: Arc<RwLock<HashMap<String, MdnsEsp32Device>>>,
    /// Discovery task control
    stop_tx: Option<mpsc::UnboundedSender<()>>,
    /// Running state
    is_running: bool,
}

impl MdnsDiscovery {
    /// Create new mDNS discovery service
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            mdns_daemon: None,
            discovered_devices: Arc::new(RwLock::new(HashMap::new())),
            stop_tx: None,
            is_running: false,
        })
    }
    
    /// Start mDNS discovery for ESP32 devices
    pub async fn start_discovery<F>(
        &mut self,
        device_callback: F,
    ) -> Result<(), String>
    where
        F: Fn(MdnsEsp32Device) + Send + Sync + 'static,
    {
        if self.is_running {
            return Err("mDNS discovery already running".to_string());
        }
        
        // Create mDNS daemon
        let mdns_daemon = ServiceDaemon::new()
            .map_err(|e| format!("Failed to create mDNS daemon: {}", e))?;
        
        self.mdns_daemon = Some(mdns_daemon);
        self.is_running = true;
        
        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel();
        self.stop_tx = Some(stop_tx);
        
        let discovered_devices = Arc::clone(&self.discovered_devices);
        let callback = Arc::new(device_callback);
        
        // Clone mdns_daemon for the task
        let mdns_daemon = self.mdns_daemon.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            info!("Starting mDNS discovery for ESP32 devices...");
            crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", "STARTING_MDNS_DISCOVERY");

            // Browse for Arduino OTA services
            let receiver = match mdns_daemon.browse("_arduino._tcp.local.") {
                Ok(receiver) => {
                    crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", "ARDUINO_BROWSE_SUCCESS");
                    receiver
                },
                Err(e) => {
                    error!("Failed to start mDNS browse: {}", e);
                    crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("ARDUINO_BROWSE_FAILED: {}", e));
                    return;
                }
            };

            // Also browse for HTTP services (some ESP32s might use this)
            let http_receiver = mdns_daemon.browse("_http._tcp.local.").ok();

            info!("mDNS discovery started, listening for ESP32 devices...");
            crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", "MDNS_LISTENING_FOR_DEVICES");
            
            loop {
                tokio::select! {
                    // Check for stop signal
                    _ = stop_rx.recv() => {
                        info!("Stopping mDNS discovery");
                        break;
                    }
                    
                    // Handle Arduino OTA service events
                    event = async {
                        match receiver.recv() {
                            Ok(event) => Some(("arduino", event)),
                            Err(_) => None,
                        }
                    } => {
                        if let Some((service_type, event)) = event {
                            crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("RECEIVED_SERVICE_EVENT: {} - {:?}", service_type, event));
                            Self::handle_service_event(
                                event,
                                service_type,
                                Arc::clone(&discovered_devices),
                                Arc::clone(&callback)
                            ).await;
                        }
                    }
                    
                    // Handle HTTP service events (if available)
                    event = async {
                        if let Some(ref http_receiver) = http_receiver {
                            match http_receiver.recv() {
                                Ok(event) => Some(("http", event)),
                                Err(_) => None,
                            }
                        } else {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            None
                        }
                    } => {
                        if let Some((service_type, event)) = event {
                            crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("RECEIVED_SERVICE_EVENT: {} - {:?}", service_type, event));
                            Self::handle_service_event(
                                event,
                                service_type,
                                Arc::clone(&discovered_devices),
                                Arc::clone(&callback)
                            ).await;
                        }
                    }
                }
            }
        });
        
        info!("mDNS discovery service started");
        Ok(())
    }
    
    /// Stop mDNS discovery
    pub async fn stop_discovery(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
            self.is_running = false;
            
            // Clean up mDNS daemon
            if let Some(daemon) = self.mdns_daemon.take() {
                daemon.shutdown().ok();
            }
            
            info!("mDNS discovery service stopped");
        }
    }
    
    /// Handle mDNS service events
    async fn handle_service_event<F>(
        event: ServiceEvent,
        service_type: &str,
        discovered_devices: Arc<RwLock<HashMap<String, MdnsEsp32Device>>>,
        callback: Arc<F>,
    ) 
    where
        F: Fn(MdnsEsp32Device) + Send + Sync + 'static,
    {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                let hostname = info.get_hostname().to_string();
                let addresses: Vec<IpAddr> = info.get_addresses()
                    .iter()
                    .cloned()
                    .collect();
                let port = info.get_port();

                crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("SERVICE_RESOLVED: {} ({}:{}) - {} addresses", hostname, hostname, port, addresses.len()));

                // Parse TXT records
                let mut txt_records = HashMap::new();
                let properties = info.get_properties();
                trace!("Parsing TXT records for {}: {} properties found", hostname, properties.len());
                crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("PARSING_TXT_RECORDS: {} - {} properties", hostname, properties.len()));

                for property in properties.iter() {
                    let key = property.key();
                    if let Some(value) = property.val() {
                        if let Ok(value_str) = std::str::from_utf8(value) {
                            trace!("TXT Record: {} = {}", key, value_str);
                            txt_records.insert(key.to_string(), value_str.to_string());
                        }
                    }
                }

                // Filter for ESP32 devices (check if hostname or TXT records indicate ESP32)
                let is_esp32 = Self::is_esp32_device(&hostname, &txt_records, service_type);
                crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("IS_ESP32_CHECK: {} - result: {}", hostname, is_esp32));

                if is_esp32 {
                    let device = MdnsEsp32Device {
                        hostname: hostname.clone(),
                        ip_addresses: addresses.clone(),
                        port,
                        txt_records: txt_records.clone(),
                        service_name: format!("_{}._{}.local.", service_type, "tcp"),
                    };
                    
                    // Add to cache only if it's new. Log info only when a new device is inserted.
                    let mut was_new = false;
                    {
                        let mut devices = discovered_devices.write().await;
                        if !devices.contains_key(&hostname) {
                            devices.insert(hostname.clone(), device.clone());
                            was_new = true;
                        }
                    }

                    if was_new {
                        info!("New ESP32 device discovered: {} at {:?}:{}", hostname, addresses, port);
                        crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("NEW_ESP32_DISCOVERED: {} at {:?}:{}", hostname, addresses, port));
                        // Call callback for new device
                        crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("CALLING_DEVICE_CALLBACK: {}", hostname));
                        callback(device);
                        crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("DEVICE_CALLBACK_COMPLETED: {}", hostname));
                    } else {
                        // For existing devices, only trace (no noisy logs)
                        crate::debug_logger::DebugLogger::log_event("MDNS_DISCOVERY", &format!("EXISTING_ESP32_UPDATED: {}", hostname));
                        trace!("Updated/refresh ESP32 device seen: {}", hostname);
                    }
                } else {
                    trace!("Ignoring non-ESP32 device: {} (service: {})", hostname, service_type);
                }
            }
            ServiceEvent::ServiceRemoved(typ, name) => {
                trace!("Service removed: {} {}", typ, name);
                // Optionally remove from cache based on name
                let mut devices = discovered_devices.write().await;
                devices.retain(|_, device| device.service_name != format!("{}.{}", name, typ));
            }
            _ => {
                // Handle other events if needed
                trace!("Other mDNS event: {:?}", event);
            }
        }
    }
    
    /// Determine if a discovered device is an ESP32
    fn is_esp32_device(hostname: &str, txt_records: &HashMap<String, String>, service_type: &str) -> bool {
        // Filter out our own ESP32 Manager Server
        let hostname_lower = hostname.to_lowercase();
        if hostname_lower.contains("esp-server") {
            return false;
        }

        // Check if device has a MAC address in TXT records (real ESP32s should have this)
        let has_mac_address = txt_records.contains_key("mac") ||
                             txt_records.contains_key("MAC") ||
                             txt_records.contains_key("macAddress");

        // Only accept devices that have MAC addresses
        if !has_mac_address {
            return false;
        }

        // Check hostname for ESP32 indicators
        let hostname_indicators = [
            "esp32", "esp", "arduino", "nodemcu", "wemos", "devkit"
        ];

        let hostname_matches = hostname_indicators.iter()
            .any(|indicator| hostname_lower.contains(indicator));

        // For Arduino OTA service, assume it's likely an ESP32
        if service_type == "arduino" {
            return true;
        }

        // Check TXT records for ESP32/Arduino indicators
        let txt_indicators = txt_records.values()
            .any(|value| {
                let value_lower = value.to_lowercase();
                value_lower.contains("esp32") ||
                value_lower.contains("arduino") ||
                value_lower.contains("espressif")
            });

        // Accept if hostname matches OR TXT records indicate ESP32
        hostname_matches || txt_indicators
    }
}

impl Drop for MdnsDiscovery {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        
        if let Some(daemon) = self.mdns_daemon.take() {
            daemon.shutdown().ok();
        }
    }
}

/// Create a new mDNS discovery service
pub fn create_mdns_discovery() -> Result<MdnsDiscovery, String> {
    MdnsDiscovery::new()
}