// ESP32 device manager - handles multiple ESP32 connections and integrates with device store

use crate::esp32_connection::{Esp32Connection};
use crate::esp32_types::{
    Esp32Command, Esp32Event, Esp32DeviceConfig, ConnectionState, Esp32Result, Esp32Error
};
use crate::device_store::{SharedDeviceStore, DeviceEventStore};
use crate::events::DeviceEvent;
use crate::debug_logger::DebugLogger;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout, Duration, interval};
use tracing::{info, warn, error, debug};

// ============================================================================
// ESP32 DEVICE MANAGER
// ============================================================================

/// Type of device connection - tracks whether device is UART or TCP/UDP
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceConnectionType {
    Uart,
    TcpUdp,
}

/// Manages multiple ESP32 device connections and integrates with the device store
#[derive(Debug)]
pub struct Esp32Manager {
    /// Map of device_id -> ESP32 connection
    connections: Arc<RwLock<HashMap<String, Arc<Mutex<Esp32Connection>>>>>,
    /// Device configurations
    device_configs: Arc<RwLock<HashMap<String, Esp32DeviceConfig>>>,
    /// Shared device store for event management
    device_store: SharedDeviceStore,
    /// Central UDP listener for all ESP32 devices
    central_udp_socket: Arc<Mutex<Option<UdpSocket>>>,
    /// Map of IP -> device_id for UDP message routing
    ip_to_device_id: Arc<RwLock<HashMap<IpAddr, String>>>,
    /// Global mutex to prevent race conditions during device connections
    connection_mutex: Arc<Mutex<()>>,
    /// Unified activity tracking for UDP and UART devices (not TCP)
    unified_activity_tracker: Arc<RwLock<HashMap<String, Instant>>>,
    /// Unified connection state tracking to prevent redundant events (device_id -> is_connected)
    unified_connection_states: Arc<RwLock<HashMap<String, bool>>>,
    /// Map of device_id -> DeviceConnectionType to track UART vs TCP/UDP devices
    device_connection_types: Arc<RwLock<HashMap<String, DeviceConnectionType>>>,
}

/// Metadata about the message source
#[derive(Debug, Clone)]
pub enum MessageSource {
    Uart,
    Tcp { ip: String, port: u16 },
    Udp { ip: String, port: u16 },
}

impl Esp32Manager {
    /// Create new ESP32 manager
    pub fn new(device_store: SharedDeviceStore) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            device_configs: Arc::new(RwLock::new(HashMap::new())),
            device_store,
            central_udp_socket: Arc::new(Mutex::new(None)),
            ip_to_device_id: Arc::new(RwLock::new(HashMap::new())),
            connection_mutex: Arc::new(Mutex::new(())),
            unified_activity_tracker: Arc::new(RwLock::new(HashMap::new())),
            unified_connection_states: Arc::new(RwLock::new(HashMap::new())),
            device_connection_types: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Start the ESP32 manager background tasks
    pub async fn start(&self) {
        info!("Starting ESP32 Manager");

        // Start central UDP listener immediately
        if let Err(e) = self.start_central_udp_listener().await {
            error!("Failed to start central UDP listener: {}", e);
        }



        // Start unified timeout monitoring task (for UDP and UART, not TCP)
        self.start_unified_timeout_monitor().await;

        info!("ESP32 Manager started");
    }
    
    /// Add a new ESP32 device configuration
    pub async fn add_device(&self, config: Esp32DeviceConfig) -> Esp32Result<()> {
        let device_id = config.device_id.clone();
        info!("Adding ESP32 device: {} ({}:{})",
               device_id, config.ip_address, config.tcp_port);
        crate::debug_logger::DebugLogger::log_device_add(&device_id);

        // Check if device already exists
        {
            let connections = self.connections.read().await;
            if connections.contains_key(&device_id) {
                info!("ESP32 device {} already exists, updating configuration only", device_id);
                crate::debug_logger::DebugLogger::log_device_already_exists(&device_id);

                // Update configuration but keep existing connection
                let mut configs = self.device_configs.write().await;
                configs.insert(device_id.clone(), config.clone());

                return Ok(());
            }
        }

        // Store configuration
        {
            let mut configs = self.device_configs.write().await;
            configs.insert(device_id.clone(), config.clone());
        }

        // Register as TCP/UDP device in connection type map
        {
            let mut conn_types = self.device_connection_types.write().await;
            conn_types.insert(device_id.clone(), DeviceConnectionType::TcpUdp);
            debug!("Registered device {} as TCP/UDP type", device_id);
        }

        // Create connection with direct manager event sender - SIMPLIFIED SYSTEM
        info!("Creating ESP32Connection for device {} with direct manager event sender", device_id);

        // Use manager's bypass event sender directly to avoid complex forwarding layers
        let device_event_sender = self.create_direct_device_sender(device_id.clone());

        info!("Direct event sender created for device {} - closed: {}", device_id, device_event_sender.is_closed());
        let connection = Esp32Connection::new(
            config,
            device_event_sender,
            self.device_store.clone(),
            self.get_unified_connection_states(),
            self.get_device_connection_types()
        );

        {
            let mut connections = self.connections.write().await;
            crate::debug_logger::DebugLogger::log_device_manager_state(&device_id, "ADDING to connections HashMap");
            connections.insert(device_id.clone(), Arc::new(Mutex::new(connection)));
            crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECTION_STORED_IN_HASHMAP: {}", device_id));
            crate::debug_logger::DebugLogger::log_device_manager_state(&device_id, "ADDED to connections HashMap");
        }

        info!("ESP32 device {} added successfully", device_id);
        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("ESP32_DEVICE_ADDED_SUCCESS: {}", device_id));
        Ok(())
    }
    
    /// Remove ESP32 device
    pub async fn remove_device(&self, device_id: &str) -> Esp32Result<()> {
        info!("Removing ESP32 device: {}", device_id);
        
        // Disconnect if connected
        if let Err(e) = self.disconnect_device(device_id).await {
            warn!("Error disconnecting device {} during removal: {}", device_id, e);
        }
        
        // Remove from collections
        {
            let mut connections = self.connections.write().await;
            crate::debug_logger::DebugLogger::log_device_manager_state(device_id, "REMOVING from connections HashMap");
            connections.remove(device_id);
            crate::debug_logger::DebugLogger::log_device_manager_state(device_id, "REMOVED from connections HashMap");
        }


        {
            let mut configs = self.device_configs.write().await;
            configs.remove(device_id);
        }
        
        info!("ESP32 device {} removed", device_id);
        Ok(())
    }
    
    /// Connect to ESP32 device
    pub async fn connect_device(&self, device_id: &str) -> Esp32Result<()> {
        info!("DEVICE CONNECTION DEBUG: Starting connection process for device: {}", device_id);
        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECT_DEVICE_START: {}", device_id));

        // Use global mutex to prevent race conditions between multiple connection attempts
        let _connection_guard = self.connection_mutex.lock().await;
        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECT_DEVICE_MUTEX_ACQUIRED: {}", device_id));

        // First, check if we need to recreate the connection with a fresh direct sender
        let needs_recreation = {
            let connections = self.connections.read().await;
            if let Some(connection_arc) = connections.get(device_id) {
                let connection = connection_arc.lock().await;
                let current_state = connection.get_connection_state().await;
                match current_state {
                    ConnectionState::Connected => {
                        info!("DEVICE CONNECTION DEBUG: Device {} already connected - skipping", device_id);
                        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("ALREADY_CONNECTED_SKIP: {}", device_id));
                        return Ok(());
                    }
                    ConnectionState::Connecting => {
                        info!("DEVICE CONNECTION DEBUG: Device {} is in connecting state (likely after reset) - attempting reconnect", device_id);
                        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECTING_STATE_RECONNECT: {}", device_id));
                        false // Use existing connection and try to reconnect
                    }
                    ConnectionState::Disconnected | ConnectionState::Failed(_) => {
                        info!("DEVICE CONNECTION DEBUG: Device {} is disconnected/failed - recreating connection", device_id);
                        crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("RECREATING_CONNECTION: {}", device_id));
                        true // Recreate connection
                    }
                }
            } else {
                false // No connection exists
            }
        };

        if needs_recreation {
            info!("DEVICE CONNECTION DEBUG: Recreating ESP32Connection with fresh direct sender for device: {}", device_id);

            // Get device config
            let config = {
                let configs = self.device_configs.read().await;
                configs.get(device_id).cloned().ok_or_else(|| {
                    Esp32Error::DeviceNotFound(format!("Device config not found for {}", device_id))
                })?
            };

            // Create new ESP32Connection with fresh direct sender
            let direct_sender = self.create_direct_device_sender(device_id.to_string());
            let new_connection = Esp32Connection::new(
                config.clone(),
                direct_sender,
                self.device_store.clone(),
                self.get_unified_connection_states(),
                self.get_device_connection_types()
            );
            let connection_arc = Arc::new(Mutex::new(new_connection));

            // Replace the connection
            {
                let mut connections = self.connections.write().await;
                connections.insert(device_id.to_string(), connection_arc.clone());
            }

            info!("DEVICE CONNECTION DEBUG: ESP32Connection recreated for device: {}", device_id);
        }

        let connections = self.connections.read().await;
        if let Some(connection_arc) = connections.get(device_id) {
            info!("DEVICE CONNECTION DEBUG: Found connection for device: {}", device_id);
            crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECTION_FOUND: {}", device_id));

            let mut connection = connection_arc.lock().await;

            info!("DEVICE CONNECTION DEBUG: Attempting TCP connection for device: {}", device_id);
            crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("ATTEMPTING_TCP_CONNECTION: {}", device_id));

            match connection.connect().await {
                Ok(()) => {
                    info!("DEVICE CONNECTION DEBUG: TCP connection established for device: {}", device_id);
                    crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("TCP_CONNECTION_SUCCESS: {}", device_id));
                },
                Err(e) => {
                    error!("DEVICE CONNECTION DEBUG: TCP connection failed for device: {} - Error: {}", device_id, e);
                    crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("TCP_CONNECTION_FAILED: {} - Error: {}", device_id, e));
                    return Err(e);
                }
            }

            // Register device for central UDP routing
            let config = {
                let configs = self.device_configs.read().await;
                configs.get(device_id).cloned()
            };

            if let Some(ref config) = config {
                crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("REGISTERING_UDP_ROUTING: {} -> {}", device_id, config.ip_address));
                self.register_esp32_for_udp(device_id.to_string(), config.ip_address).await;

                // Initialize unified activity tracking for connected device
                {
                    let mut tracker = self.unified_activity_tracker.write().await;
                    tracker.insert(device_id.to_string(), Instant::now());
                    info!("Unified activity tracking initialized for device: {}", device_id);
                }

                // Mark device as connected in unified connection states
                {
                    let mut states = self.unified_connection_states.write().await;
                    states.insert(device_id.to_string(), true);
                    info!("Unified connection state set to connected for device: {}", device_id);
                }
            }

            info!("DEVICE CONNECTION DEBUG: Successfully connected to ESP32 device: {}", device_id);
            info!("DEVICE CONNECTION DEBUG: Connection status events should now be sent to frontend for device: {}", device_id);
            crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("CONNECT_DEVICE_SUCCESS: {}", device_id));

            // WORKAROUND: Send connection status event directly through manager
            // This ensures frontend gets notified even if ESP32Connection event sender is closed
            if let Some(config) = config {
                let device_event = crate::events::DeviceEvent::esp32_connection_status(
                    device_id.to_string(),
                    true, // connected
                    config.ip_address.to_string(),
                    config.tcp_port,
                    config.udp_port,
                );

                if let Err(e) = self.device_store.add_event(
                    device_id.to_string(),
                    device_event,
                    "ESP32_MANAGER".to_string(),
                    "SYSTEM_CONNECTION".to_string(),
                ).await {
                    error!("ESP32MANAGER DEBUG: Failed to send manual connection status event for device {}: {}", device_id, e);
                } else {
                    info!("ESP32MANAGER DEBUG: Manual connection status event sent successfully for device {}", device_id);
                }
            }

            Ok(())
        } else {
            crate::debug_logger::DebugLogger::log_event("ESP32_MANAGER", &format!("DEVICE_NOT_FOUND: {}", device_id));
            Err(Esp32Error::DeviceNotFound(device_id.to_string()))
        }
    }
    
    /// Disconnect from ESP32 device
    pub async fn disconnect_device(&self, device_id: &str) -> Esp32Result<()> {
        info!("Disconnecting from ESP32 device: {}", device_id);

        let connections = self.connections.read().await;
        if let Some(connection_arc) = connections.get(device_id) {
            let mut connection = connection_arc.lock().await;

            // Unregister from UDP routing first
            let config = {
                let configs = self.device_configs.read().await;
                configs.get(device_id).cloned()
            };

            if let Some(config) = config {
                self.unregister_esp32_from_udp(&config.ip_address).await;
            }

            connection.disconnect().await?;
            info!("Successfully disconnected from ESP32 device: {}", device_id);
            Ok(())
        } else {
            Err(Esp32Error::DeviceNotFound(device_id.to_string()))
        }
    }
    
    /// Send command to ESP32 device
    pub async fn send_command(&self, device_id: &str, command: Esp32Command) -> Esp32Result<()> {
        debug!("Sending command to ESP32 device {}: {:?}", device_id, command);
        
        let connections = self.connections.read().await;
        if let Some(connection_arc) = connections.get(device_id) {
            let connection = connection_arc.lock().await;
            connection.send_command(command).await?;
            debug!("Command sent successfully to ESP32 device: {}", device_id);
            Ok(())
        } else {
            Err(Esp32Error::DeviceNotFound(device_id.to_string()))
        }
    }
    
    /// Get connection state of ESP32 device
    pub async fn get_device_state(&self, device_id: &str) -> Option<ConnectionState> {
        let connections = self.connections.read().await;
        if let Some(connection_arc) = connections.get(device_id) {
            let connection = connection_arc.lock().await;
            Some(connection.get_connection_state().await)
        } else {
            None
        }
    }
    
    /// Get all configured ESP32 devices
    pub async fn get_all_devices(&self) -> Vec<Esp32DeviceConfig> {
        let configs = self.device_configs.read().await;
        configs.values().cloned().collect()
    }
    
    /// Get device configuration
    pub async fn get_device_config(&self, device_id: &str) -> Option<Esp32DeviceConfig> {
        let configs = self.device_configs.read().await;
        configs.get(device_id).cloned()
    }

    /// Get device connection type (UART vs TCP/UDP)
    pub async fn get_device_connection_type(&self, device_id: &str) -> Option<DeviceConnectionType> {
        let conn_types = self.device_connection_types.read().await;
        conn_types.get(device_id).copied()
    }

    /// Get reference to device connection types map (for sharing with other components)
    pub fn get_device_connection_types(&self) -> Arc<RwLock<HashMap<String, DeviceConnectionType>>> {
        Arc::clone(&self.device_connection_types)
    }
    
    /// Auto-discover ESP32 devices (placeholder for future UDP discovery)
    pub async fn discover_devices(&self) -> Esp32Result<Vec<Esp32DeviceConfig>> {
        // TODO: Implement UDP broadcast discovery like UdpSearcher.cs
        // For now return empty list
        info!("ESP32 device discovery not yet implemented");
        Ok(Vec::new())
    }
    
    // ========================================================================
    // INTEGRATION WITH DEVICE STORE
    // ========================================================================
    
    /// Handle ESP32 command from WebSocket client (via device store)
    pub async fn handle_websocket_command(
        &self,
        device_id: &str,
        command_data: serde_json::Value,
        user_id: &str,
        client_id: &str,
    ) -> Esp32Result<()> {
        debug!("Handling WebSocket command for ESP32 device {}: {:?}", device_id, command_data);
        
        // Parse command from JSON
        let command = self.parse_websocket_command(command_data)?;
        
        // Send command to ESP32
        self.send_command(device_id, command.clone()).await?;
        
        // Create device event for logging/broadcasting
        let device_event = DeviceEvent::esp32_command(
            device_id.to_string(),
            serde_json::to_value(command)?,
        );
        
        // Add event to device store (this will broadcast to all connected clients)
        if let Err(e) = self.device_store.add_event(
            device_id.to_string(),
            device_event,
            user_id.to_string(),
            client_id.to_string(),
        ).await {
            error!("Failed to add ESP32 command event to device store: {}", e);
        }
        
        Ok(())
    }
    
    /// Parse WebSocket command data into ESP32 command
    fn parse_websocket_command(&self, data: serde_json::Value) -> Esp32Result<Esp32Command> {
        // Handle setVariable command
        if let Some(set_var) = data.get("setVariable") {
            if let (Some(name), Some(value)) = (set_var.get("name"), set_var.get("value")) {
                if let (Some(name_str), Some(value_num)) = (name.as_str(), value.as_u64()) {
                    return Ok(Esp32Command::set_variable(
                        name_str.to_string(),
                        value_num as u32,
                    ));
                }
            }
        }
        
        // Handle startOption command
        if let Some(start_option) = data.get("startOption") {
            if let Some(option_str) = start_option.as_str() {
                return Ok(Esp32Command::start_option(option_str.to_string()));
            }
        }
        
        // Handle reset command
        if data.get("reset").is_some() {
            return Ok(Esp32Command::reset());
        }
        
        // Handle getStatus command
        if data.get("getStatus").is_some() {
            return Ok(Esp32Command::get_status());
        }
        
        Err(Esp32Error::InvalidCommand(format!("Unknown command: {:?}", data)))
    }
    
    // ========================================================================
    // EVENT PROCESSING
    // ========================================================================
    

    /// Create a direct device event sender - SIMPLIFIED VERSION
    /// This sends events directly to the DeviceStore, bypassing all intermediate processing
    fn create_direct_device_sender(&self, device_id: String) -> mpsc::UnboundedSender<Esp32Event> {
        info!("Creating direct device sender for {}", device_id);

        // Create a simple channel that sends events directly to DeviceStore
        let (tx, mut rx) = mpsc::unbounded_channel();
        let device_store = self.device_store.clone();

        // Spawn a simple forwarding task that sends directly to DeviceStore
        tokio::spawn(async move {
            info!("DIRECT SENDER: Started direct forwarding task for device {}", device_id);

            while let Some(esp32_event) = rx.recv().await {
                // Convert ESP32 event to DeviceEvent and send directly to DeviceStore
                if let Err(e) = Self::handle_esp32_event(&device_store, &device_id, esp32_event).await {
                    warn!("DIRECT SENDER: Failed to handle event for device {}: {}", device_id, e);
                }
            }

            info!("DIRECT SENDER: Direct forwarding task ended for device {}", device_id);
        });

        tx
    }



    /// Handle ESP32 event by converting it to DeviceEvent and storing it
    async fn handle_esp32_event(
        device_store: &DeviceEventStore,
        device_id: &str,
        esp32_event: Esp32Event,
    ) -> Result<(), String> {
        debug!("Processing ESP32 event for device {}: {:?}", device_id, esp32_event);

        // Use device_id as-is (with hyphens for MAC addresses) for consistent key usage
        debug!("Using device ID '{}' for WebSocket broadcasting", device_id);

        // Convert ESP32 event to DeviceEvent using device_id
        let device_event = match esp32_event {
            Esp32Event::VariableUpdate { name, value } => {
                DeviceEvent::esp32_variable_update(device_id.to_string(), name, value)
            }
            Esp32Event::StartOptions { options } => {
                DeviceEvent::esp32_start_options(device_id.to_string(), options)
            }
            Esp32Event::ChangeableVariables { variables } => {
                let vars_json: Vec<serde_json::Value> = variables.into_iter().map(|v| {
                    serde_json::json!({ "name": v.name, "value": v.value })
                }).collect();
                DeviceEvent::esp32_changeable_variables(device_id.to_string(), vars_json)
            }
            Esp32Event::UdpBroadcast { message, from_ip, from_port } => {
                DeviceEvent::esp32_udp_broadcast(device_id.to_string(), message, from_ip, from_port)
            }
            Esp32Event::ConnectionStatus { connected, device_ip, tcp_port, udp_port } => {
                info!("ESP32 EVENT PROCESSING DEBUG: Processing connection status event for device {}: connected={}, ip={}, tcp_port={}, udp_port={}",
                      device_id, connected, device_ip, tcp_port, udp_port);
                if connected {
                    info!("ESP32 EVENT PROCESSING DEBUG: Device {} is now CONNECTED - this should update frontend to show 'Connected'", device_id);
                } else {
                    info!("ESP32 EVENT PROCESSING DEBUG: Device {} is now DISCONNECTED - this should update frontend to show 'Disconnected'", device_id);
                }
                DeviceEvent::esp32_connection_status(device_id.to_string(), connected, device_ip, tcp_port, udp_port)
            }
            Esp32Event::DeviceInfo { device_id: _, device_name, firmware_version, uptime } => {
                DeviceEvent::esp32_device_info(device_id.to_string(), device_name, firmware_version, uptime)
            }
        };
        
        // Add event to device store (this will broadcast to all connected WebSocket clients)
        // Use device_id consistently (with hyphens for MAC addresses)
        device_store.add_event(
            device_id.to_string(),
            device_event,
            "ESP32_SYSTEM".to_string(), // System user for ESP32 events
            "ESP32_INTERNAL".to_string(), // Internal client ID
        ).await?;
        
        Ok(())
    }

    // ========================================================================
    // CENTRAL UDP LISTENER
    // ========================================================================

    /// Start central UDP listener for all ESP32 devices
    async fn start_central_udp_listener(&self) -> Esp32Result<()> {
        const UDP_PORT: u16 = 3232;
        let addr = SocketAddr::from(([0, 0, 0, 0], UDP_PORT));

        let socket = UdpSocket::bind(addr)
            .await
            .map_err(|e| Esp32Error::ConnectionFailed(
                format!("Central UDP bind failed on {}: {}", addr, e)
            ))?;

        info!("Central UDP listener started on {}", addr);

        // Store socket
        {
            let mut udp_socket = self.central_udp_socket.lock().await;
            *udp_socket = Some(socket);
        }

        // Start listener task
        let socket = Arc::clone(&self.central_udp_socket);
        let ip_to_device_id = Arc::clone(&self.ip_to_device_id);
        let device_store = Arc::clone(&self.device_store);
        let unified_activity_tracker = Arc::clone(&self.unified_activity_tracker);
        let unified_connection_states = Arc::clone(&self.unified_connection_states);
        let device_connection_types = Arc::clone(&self.device_connection_types);

        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];
            info!("Central UDP listener task started");

            loop {
                let socket_guard = socket.lock().await;
                if let Some(udp_socket) = socket_guard.as_ref() {
                    match timeout(Duration::from_millis(100), udp_socket.recv_from(&mut buffer)).await {
                        Ok(Ok((bytes_read, from_addr))) => {
                            let message = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

                            // Print to terminal only (no logging)
                            println!("UDP Message from {}: {}", from_addr, message);

                            // Route message to specific ESP32 connection if registered
                            {
                                let device_map = ip_to_device_id.read().await;
                                if let Some(device_id) = device_map.get(&from_addr.ip()) {
                                    // Use unified message handler with activity tracking
                                    Self::handle_message_unified(
                                        &message,
                                        device_id,
                                        MessageSource::Udp {
                                            ip: from_addr.ip().to_string(),
                                            port: from_addr.port(),
                                        },
                                        &device_store,
                                        &unified_connection_states,
                                        Some(&unified_activity_tracker),
                                        Some(&device_connection_types),
                                    ).await;
                                } else {
                                    drop(device_map); // Drop read lock before getting write lock

                                    // Check if this looks like a TCP message that should be routed via UDP bypass
                                    if Self::is_tcp_message(&message) {
                                        if let Some(device_id) = Self::extract_device_id_from_tcp_message(&message) {
                                            // Auto-register this IP for the device
                                            {
                                                let mut device_map = ip_to_device_id.write().await;
                                                device_map.insert(from_addr.ip(), device_id.clone());
                                            }

                                            // Route the TCP message through unified handler
                                            debug!("TCP via UDP bypass: Routing message to device {} via unified handler", device_id);
                                            Self::handle_message_unified(
                                                &message,
                                                &device_id,
                                                MessageSource::Udp {
                                                    ip: from_addr.ip().to_string(),
                                                    port: from_addr.port(),
                                                },
                                                &device_store,
                                                &unified_connection_states,
                                                Some(&unified_activity_tracker),
                                                Some(&device_connection_types),
                                            ).await;
                                        }
                                    }
                                    // No logging for unregistered devices or non-TCP messages
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Central UDP receive error: {}", e);
                            sleep(Duration::from_secs(1)).await;
                        }
                        Err(_) => {
                            // Timeout, continue
                        }
                    }
                } else {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        });

        Ok(())
    }


    // ========================================================================
    // UNIFIED MESSAGE PROCESSING (UART, TCP, UDP)
    // ========================================================================

    /// Central unified message handler for all message types (UART, TCP, UDP)
    /// This ensures consistent processing regardless of the message origin
    pub async fn handle_message_unified(
        message: &str,
        device_id: &str,
        source: MessageSource,
        device_store: &SharedDeviceStore,
        connection_states: &Arc<RwLock<HashMap<String, bool>>>,
        activity_tracker: Option<&Arc<RwLock<HashMap<String, Instant>>>>,
        device_connection_types: Option<&Arc<RwLock<HashMap<String, DeviceConnectionType>>>>,
    ) {
        let source_name = match &source {
            MessageSource::Uart => "UART",
            MessageSource::Tcp { .. } => "TCP",
            MessageSource::Udp { .. } => "UDP",
        };

        // Register device connection type if provided
        if let Some(conn_types) = device_connection_types {
            let device_type = match &source {
                MessageSource::Uart => DeviceConnectionType::Uart,
                MessageSource::Tcp { .. } | MessageSource::Udp { .. } => DeviceConnectionType::TcpUdp,
            };

            let mut types_map = conn_types.write().await;
            if !types_map.contains_key(device_id) {
                types_map.insert(device_id.to_string(), device_type);
                debug!("{} MESSAGE: Registered device {} as {:?} type", source_name, device_id, device_type);
            }
        }

        // Update activity tracker for UDP and UART (not TCP)
        let should_track_activity = matches!(source, MessageSource::Uart | MessageSource::Udp { .. });
        if should_track_activity {
            if let Some(tracker) = activity_tracker {
                let mut tracker_guard = tracker.write().await;
                tracker_guard.insert(device_id.to_string(), Instant::now());
            }
        }

        // Smart connection state tracking - send event only on state change
        let should_send_connected_event = {
            let mut states = connection_states.write().await;
            let was_connected = states.get(device_id).copied().unwrap_or(false);

            if !was_connected {
                states.insert(device_id.to_string(), true);
                true
            } else {
                false
            }
        };

        // Send connection event only if state changed
        if should_send_connected_event {
            let (ip, tcp_port, udp_port) = match &source {
                MessageSource::Uart => ("0.0.0.0".to_string(), 0, 0),
                MessageSource::Tcp { ip, port } => (ip.clone(), *port, 0),
                MessageSource::Udp { ip, port } => (ip.clone(), 0, *port),
            };

            let connection_event = crate::events::DeviceEvent::esp32_connection_status(
                device_id.to_string(),
                true,
                ip,
                tcp_port,
                udp_port,
            );

            if let Err(e) = device_store.add_event(
                device_id.to_string(),
                connection_event,
                "esp32_system".to_string(),
                format!("{}_connect", source_name.to_lowercase()),
            ).await {
                error!("Failed to send {} connection event for device {}: {}", source_name, device_id, e);
            }
        }

        // Send broadcast event with actual source info
        let (ip, port) = match &source {
            MessageSource::Uart => ("0.0.0.0".to_string(), 0),
            MessageSource::Tcp { ip, port } | MessageSource::Udp { ip, port } => (ip.clone(), *port),
        };

        let broadcast_event = crate::events::DeviceEvent::esp32_udp_broadcast(
            device_id.to_string(),
            message.to_string(),
            ip,
            port,
        );
        let _ = device_store.add_event(
            device_id.to_string(),
            broadcast_event,
            "esp32_system".to_string(),
            format!("{}_message", source_name.to_lowercase()),
        ).await;

        // Parse message and extract structured data (JSON + regex fallback)
        Self::parse_and_process_message(message, device_id, device_store, source_name).await;
    }

    /// Parse message and create appropriate events
    /// Combines JSON parsing with regex fallback for legacy messages
    async fn parse_and_process_message(
        message: &str,
        device_id: &str,
        device_store: &SharedDeviceStore,
        source_name: &str,
    ) {
        // Try JSON parsing first (structured data)
        let _json_parsed = if let Ok(value) = serde_json::from_str::<serde_json::Value>(message) {
            // Handle startOptions array
            if let Some(options_array) = value.get("startOptions") {
                if let Some(options) = options_array.as_array() {
                    let mut start_options = Vec::new();
                    for option in options {
                        if let Some(option_str) = option.as_str() {
                            start_options.push(option_str.to_string());
                        }
                    }

                    if !start_options.is_empty() {
                        debug!("{}: Extracted startOptions: {:?}", source_name, start_options);
                        let start_options_event = crate::events::DeviceEvent::esp32_start_options(
                            device_id.to_string(),
                            start_options
                        );
                        let _ = device_store.add_event(
                            device_id.to_string(),
                            start_options_event,
                            "esp32_system".to_string(),
                            format!("{}_data", source_name.to_lowercase())
                        ).await;
                    }
                }
            }

            // Handle changeableVariables array
            if let Some(vars_array) = value.get("changeableVariables") {
                if let Some(vars) = vars_array.as_array() {
                    let mut variables = Vec::new();
                    for var in vars {
                        if let (Some(name), Some(value)) = (var.get("name"), var.get("value")) {
                            if let (Some(name_str), Some(value_num)) = (name.as_str(), value.as_u64()) {
                                // Basis-Variable mit name und value
                                let mut var_json = serde_json::json!({
                                    "name": name_str,
                                    "value": value_num
                                });

                                // Optional: min Wert hinzufügen
                                if let Some(min_val) = var.get("min").and_then(|v| v.as_u64()) {
                                    var_json["min"] = serde_json::json!(min_val);
                                }

                                // Optional: max Wert hinzufügen
                                if let Some(max_val) = var.get("max").and_then(|v| v.as_u64()) {
                                    var_json["max"] = serde_json::json!(max_val);
                                }

                                variables.push(var_json);
                            }
                        }
                    }

                    if !variables.is_empty() {
                        debug!("{}: Extracted changeableVariables: {:?}", source_name, variables);
                        let changeable_vars_event = crate::events::DeviceEvent::esp32_changeable_variables(
                            device_id.to_string(),
                            variables
                        );
                        let _ = device_store.add_event(
                            device_id.to_string(),
                            changeable_vars_event,
                            "esp32_system".to_string(),
                            format!("{}_data", source_name.to_lowercase())
                        ).await;
                    }
                }
            }

            // Handle device information
            if let Some(device_name) = value.get("deviceName").and_then(|v| v.as_str()) {
                let firmware_version = value.get("firmwareVersion").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let uptime = value.get("uptime").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                debug!("{}: Extracted device info - name: {}, firmware: {}, uptime: {}", source_name, device_name, firmware_version, uptime);
                let device_info_event = crate::events::DeviceEvent::esp32_device_info(
                    device_id.to_string(),
                    Some(device_name.to_string()),
                    Some(firmware_version),
                    Some(uptime as u64)
                );
                let _ = device_store.add_event(
                    device_id.to_string(),
                    device_info_event,
                    "esp32_system".to_string(),
                    format!("{}_data", source_name.to_lowercase())
                ).await;
            }

            // Handle status information
            if let Some(status) = value.get("status") {
                if let Some(status_obj) = status.as_object() {
                    if let Some(running) = status_obj.get("running").and_then(|v| v.as_bool()) {
                        debug!("{}: Device {} status - running: {}", source_name, device_id, running);
                    }

                    if let Some(memory_free) = status_obj.get("memoryFree").and_then(|v| v.as_u64()) {
                        debug!("{}: Device {} memory free: {} bytes", source_name, device_id, memory_free);
                    }
                }
            }

            // Handle legacy single variable updates with optional min/max
            // Format: {"variableName": "value", "min": 0, "max": 100}
            // or: {"variableName": 123, "min": 0, "max": 100}
            if let Some(obj) = value.as_object() {
                // Collect all fields except known metadata fields
                let skip_fields = ["device_id", "startOptions", "changeableVariables",
                                   "deviceName", "firmwareVersion", "uptime", "status"];

                // Check if this is a simple variable update with min/max
                let has_min = obj.contains_key("min");
                let has_max = obj.contains_key("max");

                if has_min || has_max {
                    // Find the variable field(s) - any field that's not min/max or metadata
                    for (key, val) in obj.iter() {
                        if key != "min" && key != "max" && !skip_fields.contains(&key.as_str()) {
                            // Extract value (string or number)
                            let value_str = if let Some(s) = val.as_str() {
                                s.to_string()
                            } else if let Some(n) = val.as_u64() {
                                n.to_string()
                            } else if let Some(n) = val.as_i64() {
                                n.to_string()
                            } else if let Some(f) = val.as_f64() {
                                f.to_string()
                            } else {
                                continue; // Skip non-primitive values
                            };

                            // Extract min/max if present
                            let min_val = obj.get("min").and_then(|v| v.as_u64());
                            let max_val = obj.get("max").and_then(|v| v.as_u64());

                            // Send variable update event with min/max metadata
                            debug!("{}: Extracted variable with min/max - name: {}, value: {}, min: {:?}, max: {:?}",
                                   source_name, key, value_str, min_val, max_val);

                            // Send as Esp32VariableUpdate with min/max
                            let variable_event = crate::events::DeviceEvent::esp32_variable_update_with_range(
                                device_id.to_string(),
                                key.clone(),
                                value_str,
                                min_val,
                                max_val,
                            );

                            let _ = device_store.add_event(
                                device_id.to_string(),
                                variable_event,
                                "esp32_system".to_string(),
                                format!("{}_data", source_name.to_lowercase())
                            ).await;
                        }
                    }
                }
            }

            true // JSON parsing succeeded
        } else {
            false // JSON parsing failed
        };

        // Parse simple key-value variable updates using regex
        // This handles messages like {"ledBlinkDelay":"1000"} or {"var":123}
        // These are valid JSON but not structured like startOptions/changeableVariables
        // Parse for string variable updates: {"name": "value"}
        let re = regex::Regex::new(r#"\{\"([^\"]+)\"\s*:\s*\"([^\"]+)\"\}"#).unwrap();
        for captures in re.captures_iter(message) {
            if let (Some(name), Some(value)) = (captures.get(1), captures.get(2)) {
                let name_str = name.as_str().trim();
                // Skip known structured fields to avoid duplicates
                if name_str != "deviceName" && name_str != "firmwareVersion"
                    && name_str != "name" && name_str != "value" {
                    let variable_event = crate::events::DeviceEvent::esp32_variable_update(
                        device_id.to_string(),
                        name_str.to_string(),
                        value.as_str().trim().to_string(),
                    );
                    let _ = device_store.add_event(
                        device_id.to_string(),
                        variable_event,
                        "esp32_system".to_string(),
                        format!("{}_data", source_name.to_lowercase())
                    ).await;
                }
            }
        }

        // Parse for numeric variable updates: {"name": 123}
        let numeric_re = regex::Regex::new(r#"\{\"([^\"]+)\"\s*:\s*(\d+)\}"#).unwrap();
        for captures in numeric_re.captures_iter(message) {
            if let (Some(name), Some(value)) = (captures.get(1), captures.get(2)) {
                let name_str = name.as_str().trim();
                // Skip known structured fields to avoid duplicates
                if name_str != "uptime" && name_str != "value" {
                    let variable_event = crate::events::DeviceEvent::esp32_variable_update(
                        device_id.to_string(),
                        name_str.to_string(),
                        value.as_str().trim().to_string(),
                    );
                    let _ = device_store.add_event(
                        device_id.to_string(),
                        variable_event,
                        "esp32_system".to_string(),
                        format!("{}_data", source_name.to_lowercase())
                    ).await;
                }
            }
        }
    }

    // ========================================================================
    // MESSAGE BYPASS FUNCTIONS
    // ========================================================================

    /// Handle TCP message - calls unified handler
    /// TCP messages do NOT use activity tracking (no timeout for TCP)
    /// but DO use unified connection states to prevent redundant events
    pub async fn handle_tcp_message_bypass(
        message: &str,
        device_id: &str,
        device_store: &SharedDeviceStore,
        unified_connection_states: &Arc<RwLock<HashMap<String, bool>>>,
        device_connection_types: &Arc<RwLock<HashMap<String, DeviceConnectionType>>>,
    ) {
        DebugLogger::log_tcp_message(device_id, "RECEIVED", message);

        Self::handle_message_unified(
            message,
            device_id,
            MessageSource::Tcp {
                ip: "0.0.0.0".to_string(),
                port: 3232,
            },
            device_store,
            unified_connection_states,  // Use shared state (prevents redundant events)
            None,  // No activity tracking for TCP (no timeout)
            Some(device_connection_types),
        ).await;
    }

    /// Check if a message looks like a TCP message with JSON structure
    fn is_tcp_message(message: &str) -> bool {
        // TCP messages from ESP32 are usually JSON with specific fields
        message.trim_start().starts_with('{') && (
            message.contains("\"startOptions\"") ||
            message.contains("\"changeableVariables\"") ||
            message.contains("\"setVariable\"") ||
            message.contains("\"startOption\"") ||
            message.contains("\"reset\"")
        )
    }

    /// Extract device ID from TCP message structure
    fn extract_device_id_from_tcp_message(_message: &str) -> Option<String> {
        // For now, assume the known device ID since we know there's only one ESP32
        // In a real system, this would parse the message to extract device info
        Some("10-20-BA-42-71-E0".to_string())
    }

    /// Register ESP32 device for UDP message routing
    pub async fn register_esp32_for_udp(&self, device_id: String, ip: IpAddr) {
        let mut device_map = self.ip_to_device_id.write().await;
        device_map.insert(ip, device_id.clone());
        info!("ESP32 {} registered for UDP routing on IP {}", device_id, ip);
    }

    /// Unregister ESP32 device from UDP message routing
    pub async fn unregister_esp32_from_udp(&self, ip: &IpAddr) {
        let mut device_map = self.ip_to_device_id.write().await;
        if let Some(device_id) = device_map.remove(ip) {
            info!("ESP32 {} unregistered from UDP routing", device_id);
        }
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Create shared ESP32 manager instance
pub fn create_esp32_manager(device_store: SharedDeviceStore) -> Arc<Esp32Manager> {
    let manager = Arc::new(Esp32Manager::new(device_store));


    manager
}




impl Esp32Manager {
    /// Start unified timeout monitoring task for UDP and UART (not TCP)
    async fn start_unified_timeout_monitor(&self) {
        let unified_activity_tracker = Arc::clone(&self.unified_activity_tracker);
        let device_configs = Arc::clone(&self.device_configs);
        let device_store = self.device_store.clone();
        let unified_connection_states = Arc::clone(&self.unified_connection_states);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5)); // Check every 5 seconds
            info!("Unified timeout monitor started (UDP and UART)");

            loop {
                interval.tick().await;

                let mut configs = device_configs.write().await;
                let mut tracker = unified_activity_tracker.write().await;
                let now = Instant::now();

                // Collect all devices from activity tracker (includes unregistered UART devices)
                let tracked_devices: Vec<String> = tracker.keys().cloned().collect();

                // Auto-register UART devices that are in tracker but not in configs
                for device_id in &tracked_devices {
                    if !configs.contains_key(device_id) {
                        info!("UNIFIED MONITOR: Auto-registering UART device: {}", device_id);
                        let uart_config = crate::esp32_types::Esp32DeviceConfig::new_uart(device_id.clone());
                        configs.insert(device_id.clone(), uart_config);
                    }
                }

                // Check each device for timeout
                // Only devices in the activity tracker are checked (UDP/UART messages update tracker)
                for (device_id, config) in configs.iter() {
                    if let Some(last_activity) = tracker.get(device_id) {
                        let elapsed = now.duration_since(*last_activity);
                        let timeout = Duration::from_secs(config.udp_timeout_seconds);

                        if elapsed > timeout {
                            warn!("UNIFIED TIMEOUT: Device {} ({:?}) has been inactive for {}s (timeout: {}s)",
                                  device_id, config.device_source, elapsed.as_secs(), config.udp_timeout_seconds);

                            // Only send disconnect event if device was connected
                            let should_send_disconnect = {
                                let mut states = unified_connection_states.write().await;
                                let was_connected = states.get(device_id).copied().unwrap_or(false);

                                if was_connected {
                                    // Mark as disconnected
                                    states.insert(device_id.clone(), false);
                                    info!("UNIFIED TIMEOUT: Device {} marked as disconnected", device_id);
                                    true
                                } else {
                                    // Already disconnected - no event needed
                                    false
                                }
                            };

                            if should_send_disconnect {
                                // Send disconnect event
                                let disconnect_event = crate::events::DeviceEvent::esp32_connection_status(
                                    device_id.clone(),
                                    false, // disconnected
                                    config.ip_address.to_string(),
                                    config.tcp_port,
                                    config.udp_port,
                                );

                                if let Err(e) = device_store.add_event(
                                    device_id.clone(),
                                    disconnect_event,
                                    "ESP32_SYSTEM".to_string(),
                                    "UNIFIED_TIMEOUT".to_string(),
                                ).await {
                                    error!("Failed to send unified timeout disconnect event for device {}: {}", device_id, e);
                                } else {
                                    info!("UNIFIED TIMEOUT: Disconnect event sent for device {}", device_id);
                                }
                            } else {
                                debug!("UNIFIED TIMEOUT: Device {} already marked as disconnected - skipping redundant event", device_id);
                            }

                            // Remove from tracker to avoid spam
                            tracker.remove(device_id);
                        }
                    }
                }
            }
        });
    }

    /// Update UDP activity for a device
    pub async fn update_udp_activity(&self, device_id: &str) {
        let mut tracker = self.unified_activity_tracker.write().await;
        tracker.insert(device_id.to_string(), Instant::now());
        debug!("Unified activity updated for device: {}", device_id);
    }

    /// Get shared connection states for external use (e.g., UART)
    pub fn get_unified_connection_states(&self) -> Arc<RwLock<HashMap<String, bool>>> {
        Arc::clone(&self.unified_connection_states)
    }

    /// Get shared activity tracker for external use (e.g., UART)
    pub fn get_unified_activity_tracker(&self) -> Arc<RwLock<HashMap<String, Instant>>> {
        Arc::clone(&self.unified_activity_tracker)
    }
}

/// Quick setup for common ESP32 device configurations
impl Esp32DeviceConfig {
    /// Create config for ESP32 with default ports
    pub fn esp32_default(device_id: String, ip: IpAddr) -> Self {
        Self::new(device_id, ip, 3232, 3232) // ESP32 uses port 3232 for both TCP and UDP
    }

    /// Create config for ESP32-S3 with default ports
    pub fn esp32_s3_default(device_id: String, ip: IpAddr) -> Self {
        Self::new(device_id, ip, 3232, 3232) // ESP32-S3 also uses port 3232
    }
}