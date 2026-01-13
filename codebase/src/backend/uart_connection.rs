// UART connection management for ESP32 devices
// Handles serial communication with multiple ESP32 devices connected via UART

use crate::device_store::SharedDeviceStore;
use crate::esp32_manager::Esp32Manager;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{info, warn, error, debug};

// ============================================================================
// UART CONNECTION MANAGER
// ============================================================================

/// UART connection settings
#[derive(Debug, Clone)]
pub struct UartSettings {
    pub port: String,
    pub baud_rate: u32,
}

impl Default for UartSettings {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud_rate: 115200,
        }
    }
}

/// Manages UART connection for ESP32 devices
pub struct UartConnection {
    /// Current UART settings
    settings: Arc<RwLock<Option<UartSettings>>>,
    /// Serial port stream
    serial_stream: Arc<RwLock<Option<SerialStream>>>,
    /// Device store for event routing
    device_store: SharedDeviceStore,
    /// Shutdown channel
    shutdown_sender: Option<mpsc::UnboundedSender<()>>,
    /// Connection status
    is_connected: Arc<RwLock<bool>>,
    /// Unified connection states (shared with ESP32Manager)
    unified_connection_states: Arc<RwLock<HashMap<String, bool>>>,
    /// Unified activity tracker (shared with ESP32Manager)
    unified_activity_tracker: Arc<RwLock<HashMap<String, std::time::Instant>>>,
    /// Device connection types map (shared with ESP32Manager)
    device_connection_types: Arc<RwLock<HashMap<String, crate::esp32_manager::DeviceConnectionType>>>,
}

impl UartConnection {
    /// Create new UART connection manager with shared state trackers
    pub fn new(
        device_store: SharedDeviceStore,
        unified_connection_states: Arc<RwLock<HashMap<String, bool>>>,
        unified_activity_tracker: Arc<RwLock<HashMap<String, std::time::Instant>>>,
        device_connection_types: Arc<RwLock<HashMap<String, crate::esp32_manager::DeviceConnectionType>>>,
    ) -> Self {
        Self {
            settings: Arc::new(RwLock::new(None)),
            serial_stream: Arc::new(RwLock::new(None)),
            device_store,
            shutdown_sender: None,
            is_connected: Arc::new(RwLock::new(false)),
            unified_connection_states,
            unified_activity_tracker,
            device_connection_types,
        }
    }

    /// Connect to UART port with given settings
    pub async fn connect(&mut self, port: String, baud_rate: u32) -> Result<(), String> {
        info!("Connecting to UART port {} at {} baud", port, baud_rate);

        // Close existing connection if any
        self.disconnect().await?;

        // Try to open serial port
        let serial_stream = tokio_serial::new(&port, baud_rate)
            .timeout(Duration::from_millis(1000))
            .open_native_async()
            .map_err(|e| format!("Failed to open UART port {}: {}", port, e))?;

        info!("UART port {} opened successfully", port);

        // Store settings and stream
        {
            let mut settings = self.settings.write().await;
            *settings = Some(UartSettings {
                port: port.clone(),
                baud_rate,
            });
        }

        {
            let mut stream = self.serial_stream.write().await;
            *stream = Some(serial_stream);
        }

        {
            let mut connected = self.is_connected.write().await;
            *connected = true;
        }

        // Start UART listener task
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
        self.shutdown_sender = Some(shutdown_tx);
        self.start_uart_listener_task(shutdown_rx).await;

        info!("UART connection established on port {}", port);
        Ok(())
    }

    /// Disconnect from UART port
    pub async fn disconnect(&mut self) -> Result<(), String> {
        info!("Disconnecting UART connection");

        // Send shutdown signal
        if let Some(shutdown_tx) = &self.shutdown_sender {
            let _ = shutdown_tx.send(());
        }

        // Close serial port
        {
            let mut stream = self.serial_stream.write().await;
            *stream = None;
        }

        {
            let mut connected = self.is_connected.write().await;
            *connected = false;
        }

        info!("UART connection closed");
        Ok(())
    }

    /// Get current connection status
    pub async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    /// Get current settings
    pub async fn get_settings(&self) -> Option<UartSettings> {
        self.settings.read().await.clone()
    }

    /// Start background task for UART message handling
    async fn start_uart_listener_task(&self, mut shutdown_rx: mpsc::UnboundedReceiver<()>) {
        let serial_stream = Arc::clone(&self.serial_stream);
        let device_store = self.device_store.clone();
        let is_connected = Arc::clone(&self.is_connected);
        let unified_connection_states = Arc::clone(&self.unified_connection_states);
        let unified_activity_tracker = Arc::clone(&self.unified_activity_tracker);
        let device_connection_types = Arc::clone(&self.device_connection_types);

        tokio::spawn(async move {
            info!("UART listener task started");

            const STX: u8 = 0x02; // Start of Text
            const ETX: u8 = 0x03; // End of Text

            let mut buffer = Vec::new();
            let mut read_buffer = vec![0u8; 1024];

            loop {
                // Check for shutdown signal
                if shutdown_rx.try_recv().is_ok() {
                    debug!("UART listener task shutting down");
                    break;
                }

                // Read from UART stream
                let mut stream_guard = serial_stream.write().await;
                if let Some(stream) = stream_guard.as_mut() {
                    // Try to read with timeout
                    use tokio::io::AsyncReadExt;

                    let read_result = tokio::time::timeout(
                        Duration::from_millis(100),
                        stream.read(&mut read_buffer)
                    ).await;

                    match read_result {
                        Ok(Ok(0)) => {
                            // Connection closed
                            warn!("UART connection closed");
                            drop(stream_guard);
                            *is_connected.write().await = false;
                            break;
                        }
                        Ok(Ok(bytes_read)) => {
                            // Got data from UART
                            buffer.extend_from_slice(&read_buffer[..bytes_read]);

                            // Process complete messages (STX...ETX)
                            while let Some(stx_pos) = buffer.iter().position(|&b| b == STX) {
                                // Look for ETX after STX
                                if let Some(etx_pos) = buffer[stx_pos + 1..].iter().position(|&b| b == ETX) {
                                    let etx_abs_pos = stx_pos + 1 + etx_pos;

                                    // Extract message between STX and ETX
                                    let message_bytes = &buffer[stx_pos + 1..etx_abs_pos];

                                    // Convert to string
                                    if let Ok(message) = String::from_utf8(message_bytes.to_vec()) {
                                        if !message.trim().is_empty() {
                                            // Process the message
                                            let device_store_clone = device_store.clone();
                                            let unified_connection_states_clone = Arc::clone(&unified_connection_states);
                                            let unified_activity_tracker_clone = Arc::clone(&unified_activity_tracker);
                                            let device_connection_types_clone = Arc::clone(&device_connection_types);
                                            let message_clone = message.trim().to_string();
                                            tokio::spawn(async move {
                                                Self::handle_uart_message(&message_clone, &device_store_clone, &unified_connection_states_clone, &unified_activity_tracker_clone, &device_connection_types_clone).await;
                                            });
                                        }
                                    } else {
                                        warn!("UART: Received invalid UTF-8 data between STX and ETX");
                                    }

                                    // Remove processed message from buffer
                                    buffer.drain(..=etx_abs_pos);
                                } else {
                                    // ETX not found yet, wait for more data
                                    // But if buffer is too large, remove data before STX
                                    if stx_pos > 0 {
                                        buffer.drain(..stx_pos);
                                    }
                                    break;
                                }
                            }

                            // If no STX found and buffer is large, clear old data
                            if buffer.len() > 2048 && !buffer.iter().any(|&b| b == STX) {
                                warn!("UART: Buffer overflow without STX, clearing buffer");
                                buffer.clear();
                            }
                        }
                        Ok(Err(e)) => {
                            // Read error
                            error!("UART read error: {}", e);
                            drop(stream_guard);
                            *is_connected.write().await = false;
                            break;
                        }
                        Err(_) => {
                            // Timeout - no data available, continue loop
                        }
                    }
                    drop(stream_guard);
                } else {
                    // No connection, wait a bit
                    sleep(Duration::from_millis(100)).await;
                }
            }

            info!("UART listener task ended");
        });
    }

    /// Handle incoming UART message with unified state tracking
    async fn handle_uart_message(
        message: &str,
        device_store: &SharedDeviceStore,
        unified_connection_states: &Arc<RwLock<HashMap<String, bool>>>,
        unified_activity_tracker: &Arc<RwLock<HashMap<String, std::time::Instant>>>,
        device_connection_types: &Arc<RwLock<HashMap<String, crate::esp32_manager::DeviceConnectionType>>>,
    ) {
        info!("UART MESSAGE RECEIVED: {}", message);

        // Parse JSON message to extract device_id
        match serde_json::from_str::<serde_json::Value>(message) {
            Ok(json) => {
                // Extract device_id from JSON
                if let Some(device_id) = json.get("device_id").and_then(|v| v.as_str()) {
                    // Check if device needs discovery and registration (first time seen)
                    let should_send_discovery_event = {
                        let states = unified_connection_states.read().await;
                        !states.contains_key(device_id)
                    };

                    // Register and send discovery event if device is new
                    if should_send_discovery_event {
                        use crate::events::DeviceEvent;
                        use chrono::Utc;

                        // Note: UART device will be auto-registered by the unified_timeout_monitor
                        // when it sees the device in unified_activity_tracker
                        info!("UART DISCOVERY: New UART device detected: {}", device_id);

                        // Send discovery event
                        let discovery_event = DeviceEvent::esp32_device_discovered(
                            device_id.to_string(),
                            "0.0.0.0".to_string(),  // UART has no IP
                            0,  // UART has no TCP port
                            0,  // UART has no UDP port
                            Utc::now().to_rfc3339(),
                            None,  // No MAC address for UART
                            Some(format!("uart-{}", device_id))  // Virtual hostname
                        );

                        let _ = device_store.add_event(
                            "system".to_string(),
                            discovery_event,
                            "esp32_system".to_string(),
                            "uart_listener".to_string()
                        ).await;

                        info!("UART DISCOVERY: Discovery event sent for device {}", device_id);
                    }

                    // Remove device_id field from JSON and forward the rest
                    let mut json_without_device_id = json.clone();
                    if let Some(obj) = json_without_device_id.as_object_mut() {
                        obj.remove("device_id");
                        let modified_message = serde_json::to_string(&json_without_device_id)
                            .unwrap_or_else(|_| message.to_string());

                        Esp32Manager::handle_message_unified(
                            &modified_message,
                            device_id,
                            crate::esp32_manager::MessageSource::Uart,
                            device_store,
                            unified_connection_states,
                            Some(unified_activity_tracker),
                            Some(device_connection_types),
                        ).await;
                    }
                } else {
                    warn!("UART message missing device_id field: {}", message);
                }
            }
            Err(e) => {
                warn!("Failed to parse UART message as JSON: {} - Error: {}", message, e);
            }
        }
    }

    /// Send command to UART device
    pub async fn send_command(&self, device_id: &str, command_json: &str) -> Result<(), String> {
        info!("Sending UART command to device {}: {}", device_id, command_json);

        let mut stream_guard = self.serial_stream.write().await;
        if let Some(stream) = stream_guard.as_mut() {
            use tokio::io::AsyncWriteExt;

            // Parse the command JSON and add device_id field
            let command_with_device_id = match serde_json::from_str::<serde_json::Value>(command_json) {
                Ok(mut cmd_value) => {
                    // Add device_id to the command JSON
                    if let Some(obj) = cmd_value.as_object_mut() {
                        obj.insert("device_id".to_string(), serde_json::Value::String(device_id.to_string()));
                    }
                    serde_json::to_string(&cmd_value)
                        .map_err(|e| format!("Failed to serialize command with device_id: {}", e))?
                }
                Err(e) => {
                    return Err(format!("Failed to parse command JSON: {}", e));
                }
            };

            info!("UART command with device_id: {}", command_with_device_id);

            // Send command with STX (0x02) at start and ETX (0x03) at end
            const STX: u8 = 0x02; // Start of Text
            const ETX: u8 = 0x03; // End of Text

            let mut message_bytes = Vec::new();
            message_bytes.push(STX);
            message_bytes.extend_from_slice(command_with_device_id.as_bytes());
            message_bytes.push(ETX);

            stream.write_all(&message_bytes)
                .await
                .map_err(|e| format!("Failed to write to UART: {}", e))?;

            stream.flush()
                .await
                .map_err(|e| format!("Failed to flush UART: {}", e))?;

            info!("UART command sent successfully to device {}", device_id);
            Ok(())
        } else {
            Err("UART connection not established".to_string())
        }
    }

    /// List available UART ports
    pub fn list_ports() -> Result<Vec<String>, String> {
        match tokio_serial::available_ports() {
            Ok(ports) => {
                let port_names: Vec<String> = ports
                    .into_iter()
                    .map(|p| p.port_name)
                    .collect();
                Ok(port_names)
            }
            Err(e) => Err(format!("Failed to list serial ports: {}", e)),
        }
    }
}

impl Drop for UartConnection {
    fn drop(&mut self) {
        info!("UART connection being dropped");

        // Send shutdown signal if we have one
        if let Some(shutdown_tx) = &self.shutdown_sender {
            let _ = shutdown_tx.send(());
        }
    }
}
