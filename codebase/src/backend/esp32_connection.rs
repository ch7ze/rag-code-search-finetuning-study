// ESP32 TCP/UDP connection management

use crate::esp32_types::{
    Esp32Command, Esp32Event, Esp32DeviceConfig, ConnectionState, Esp32Result, Esp32Error
};
use crate::device_store::SharedDeviceStore;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::time::{timeout, sleep};
use tracing::{info, warn, error, debug};

// Global reset attempt counter
static RESET_COUNTER: AtomicU32 = AtomicU32::new(0);

// ============================================================================
// ESP32 CONNECTION MANAGER
// ============================================================================

#[derive(Debug)]
pub struct Esp32Connection {
    config: Esp32DeviceConfig,
    tcp_stream: Arc<Mutex<Option<TcpStream>>>,
    connection_state: Arc<RwLock<ConnectionState>>,
    event_sender: mpsc::UnboundedSender<Esp32Event>,
    tcp_buffer: Arc<Mutex<String>>,
    shutdown_sender: Option<mpsc::UnboundedSender<()>>,
    device_store: SharedDeviceStore,
    /// Unified connection states (shared with ESP32Manager)
    unified_connection_states: Arc<RwLock<std::collections::HashMap<String, bool>>>,
    /// Device connection types map (shared with ESP32Manager)
    device_connection_types: Arc<RwLock<std::collections::HashMap<String, crate::esp32_manager::DeviceConnectionType>>>,
}

impl Esp32Connection {
    /// Create a new ESP32 connection manager
    pub fn new(
        config: Esp32DeviceConfig,
        event_sender: mpsc::UnboundedSender<Esp32Event>,
        device_store: SharedDeviceStore,
        unified_connection_states: Arc<RwLock<std::collections::HashMap<String, bool>>>,
        device_connection_types: Arc<RwLock<std::collections::HashMap<String, crate::esp32_manager::DeviceConnectionType>>>,
    ) -> Self {
        info!("ESP32CONNECTION CREATION DEBUG: Creating new ESP32Connection for device {}", config.device_id);
        crate::debug_logger::DebugLogger::log_event("ESP32_CONNECTION", &format!("NEW_CONNECTION_CREATED: {} - sender_closed: {}", config.device_id, event_sender.is_closed()));

        Self {
            config,
            tcp_stream: Arc::new(Mutex::new(None)),
            connection_state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            event_sender,
            tcp_buffer: Arc::new(Mutex::new(String::new())),
            shutdown_sender: None,
            device_store,
            unified_connection_states,
            device_connection_types,
        }
    }
    
    /// Get current connection state
    pub async fn get_connection_state(&self) -> ConnectionState {
        self.connection_state.read().await.clone()
    }
    
    /// Start connection to ESP32 (both TCP and UDP)
    pub async fn connect(&mut self) -> Esp32Result<()> {
        info!("Connecting to ESP32 device {} at {}", 
               self.config.device_id, self.config.ip_address);
        
        // Set connecting state
        {
            let mut state = self.connection_state.write().await;
            *state = ConnectionState::Connecting;
        }
        
        // Establish TCP connection (UDP is now handled centrally)
        // No individual UDP listener needed anymore
        self.connect_tcp().await?;
        
        // Start background tasks
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
        self.shutdown_sender = Some(shutdown_tx);
        
        // Start TCP listener task
        self.start_tcp_listener_task(shutdown_rx).await;
        
        // Send connection status event
        let event = Esp32Event::connection_status(
            true,
            self.config.ip_address,
            self.config.tcp_port,
            self.config.udp_port
        );
        info!("ESP32CONNECTION DEBUG: About to send connection status event (connected=true) for device {}", self.config.device_id);
        info!("ESP32CONNECTION DEBUG: Event sender channel status - is_closed: {}", self.event_sender.is_closed());
        crate::debug_logger::DebugLogger::log_event("ESP32_CONNECTION", &format!("ABOUT_TO_SEND_CONNECTION_STATUS: {} - sender_closed: {}", self.config.device_id, self.event_sender.is_closed()));

        let is_closed = self.event_sender.is_closed();
        if is_closed {
            warn!("ESP32CONNECTION DEBUG: Event sender is closed for device {}, connection status event will be skipped", self.config.device_id);
            warn!("ESP32CONNECTION DEBUG: This explains why frontend shows 'Disconnected' - event channel is closed!");
            warn!("ESP32CONNECTION DEBUG: The ESP32 is actually connected via TCP, but status events cannot be sent to frontend");
            crate::debug_logger::DebugLogger::log_esp32_connection_event_send(&self.config.device_id, is_closed, false, Some("Event sender is closed"));
        } else {
            match self.event_sender.send(event) {
                Ok(()) => {
                    info!("ESP32CONNECTION DEBUG: Connection status event sent successfully for device {}", self.config.device_id);
                    info!("ESP32CONNECTION DEBUG: Event should now flow: ESP32Connection -> EventForwardingTask -> ESP32Manager -> DeviceStore -> WebSocket -> Frontend");
                    crate::debug_logger::DebugLogger::log_esp32_connection_event_send(&self.config.device_id, is_closed, true, None);
                }
                Err(e) => {
                    error!("ESP32CONNECTION DEBUG: FAILED to send connection status event for device {}: {}", self.config.device_id, e);
                    error!("ESP32CONNECTION DEBUG: Event sender is_closed: {}", self.event_sender.is_closed());
                    error!("ESP32CONNECTION DEBUG: This means the Event Forwarding Task receiver has been dropped!");
                    error!("ESP32CONNECTION DEBUG: This explains why frontend shows 'Disconnected' - event channel is closed!");
                    crate::debug_logger::DebugLogger::log_esp32_connection_event_send(&self.config.device_id, is_closed, false, Some(&e.to_string()));
                }
            }
        }
        
        info!("Successfully connected to ESP32 device {}", self.config.device_id);
        Ok(())
    }
    
    /// Disconnect from ESP32
    pub async fn disconnect(&mut self) -> Esp32Result<()> {
        info!("Disconnecting from ESP32 device {}", self.config.device_id);
        
        // Send shutdown signal
        if let Some(shutdown_tx) = &self.shutdown_sender {
            let _ = shutdown_tx.send(());
        }
        
        // Close connections
        {
            let mut tcp = self.tcp_stream.lock().await;
            if let Some(mut stream) = tcp.take() {
                let _ = stream.shutdown().await;
            }
        }
        
        // UDP is now handled centrally
        
        // Update state
        {
            let mut state = self.connection_state.write().await;
            *state = ConnectionState::Disconnected;
        }
        
        // Send connection status event
        let event = Esp32Event::connection_status(
            false,
            self.config.ip_address,
            self.config.tcp_port,
            self.config.udp_port
        );
        info!("Sending connection status event (connected=false) for device {}", self.config.device_id);
        if let Err(e) = self.event_sender.send(event) {
            error!("Failed to send disconnect status event for device {}: {}", self.config.device_id, e);
        } else {
            info!("Disconnect status event sent successfully for device {}", self.config.device_id);
        }
        
        info!("Disconnected from ESP32 device {}", self.config.device_id);
        Ok(())
    }
    
    /// Send command to ESP32 via TCP
    pub async fn send_command(&self, command: Esp32Command) -> Esp32Result<()> {
        debug!("Sending command to ESP32 {}: {:?}", self.config.device_id, command);

        // Check if this is a reset command (which will close the TCP connection)
        let is_reset_command = matches!(command, Esp32Command::Reset { .. });
        let reset_attempt_number = if is_reset_command {
            let attempt = RESET_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
            info!("RESET COMMAND: ESP32 {} will reset and close TCP connection - this is expected behavior (attempt #{})", self.config.device_id, attempt);
            crate::debug_logger::DebugLogger::log_reset_attempt(&self.config.device_id, attempt);
            attempt
        } else {
            0
        };

        let json_str = command.to_json()?;
        let command_name = format!("{:?}", command);

        // Log command attempt to debug file
        crate::debug_logger::DebugLogger::log_tcp_command_send(&self.config.device_id, &command_name, false); // Will be updated below

        let mut tcp = self.tcp_stream.lock().await;
        if let Some(stream) = tcp.as_mut() {
            // TCP connection is available - update log
            crate::debug_logger::DebugLogger::log_tcp_command_send(&self.config.device_id, &command_name, true);
            crate::debug_logger::DebugLogger::log_tcp_connection_status(&self.config.device_id, "AVAILABLE", "TCP stream exists, attempting to send command");

            // Send the command
            crate::debug_logger::DebugLogger::log_tcp_message(&self.config.device_id, "SENT", &json_str);
            let write_result = stream.write_all(json_str.as_bytes()).await;
            if let Err(e) = write_result {
                if is_reset_command {
                    info!("RESET COMMAND: Write failed for device {} (expected during reset): {}", self.config.device_id, e);
                    crate::debug_logger::DebugLogger::log_tcp_command_success(&self.config.device_id, &format!("{} (reset - write failed as expected)", command_name));
                    crate::debug_logger::DebugLogger::log_reset_success(&self.config.device_id, reset_attempt_number);
                    return Ok(()); // Reset commands are expected to fail during write/flush
                } else {
                    crate::debug_logger::DebugLogger::log_tcp_command_failed(&self.config.device_id, &command_name, &format!("write failed: {}", e));
                    return Err(e.into());
                }
            }

            // Flush the command
            let flush_result = stream.flush().await;
            if let Err(e) = flush_result {
                if is_reset_command {
                    info!("RESET COMMAND: Flush failed for device {} (expected during reset): {}", self.config.device_id, e);
                    crate::debug_logger::DebugLogger::log_tcp_command_success(&self.config.device_id, &format!("{} (reset - flush failed as expected)", command_name));
                    crate::debug_logger::DebugLogger::log_reset_success(&self.config.device_id, reset_attempt_number);
                    return Ok(()); // Reset commands are expected to fail during write/flush
                } else {
                    crate::debug_logger::DebugLogger::log_tcp_command_failed(&self.config.device_id, &command_name, &format!("flush failed: {}", e));
                    return Err(e.into());
                }
            }

            debug!("Command sent successfully: {}", json_str);
            crate::debug_logger::DebugLogger::log_tcp_command_success(&self.config.device_id, &command_name);

            // For reset commands, close TCP stream but keep connection ready for reconnect
            if is_reset_command {
                info!("RESET COMMAND: Closing TCP stream for device {} after reset (keeping connection alive for reconnect)", self.config.device_id);
                crate::debug_logger::DebugLogger::log_reset_success(&self.config.device_id, reset_attempt_number);
                *tcp = None; // Close our side of the connection

                // Update connection state to Connecting (ready for reconnect) instead of Disconnected
                {
                    let mut state = self.connection_state.write().await;
                    *state = ConnectionState::Connecting; // This prevents the connection from being removed from HashMap
                }

                // Do NOT send disconnect event for reset commands - this is a temporary state
                // The ESP32 will reconnect automatically and we want to keep the connection object alive
                info!("RESET COMMAND: TCP stream closed for device {}, connection kept alive for automatic reconnect", self.config.device_id);
            }

            Ok(())
        } else {
            // No TCP connection available
            crate::debug_logger::DebugLogger::log_tcp_connection_status(&self.config.device_id, "NOT_AVAILABLE", "TCP stream is None, attempting reconnection");
            crate::debug_logger::DebugLogger::log_tcp_reconnect_attempt(&self.config.device_id, "send_command - no TCP connection");

            debug!("No TCP connection available for device {}, attempting reconnection", self.config.device_id);
            drop(tcp); // Release the lock before reconnecting

            // Attempt to reconnect
            match self.connect_tcp().await {
                Ok(()) => {
                    crate::debug_logger::DebugLogger::log_tcp_reconnect_result(&self.config.device_id, true, None);
                }
                Err(e) => {
                    crate::debug_logger::DebugLogger::log_tcp_reconnect_result(&self.config.device_id, false, Some(&e.to_string()));
                    return Err(e);
                }
            }

            // Send connection status event to notify clients
            let event = Esp32Event::connection_status(
                true,
                self.config.ip_address,
                self.config.tcp_port,
                self.config.udp_port
            );
            if let Err(e) = self.event_sender.send(event) {
                warn!("Failed to send reconnection status event for device {}: {}", self.config.device_id, e);
            } else {
                debug!("Reconnection status event sent for device {}", self.config.device_id);
            }

            // Try sending the command again with the new connection
            let mut tcp = self.tcp_stream.lock().await;
            if let Some(stream) = tcp.as_mut() {
                crate::debug_logger::DebugLogger::log_tcp_connection_status(&self.config.device_id, "AVAILABLE_AFTER_RECONNECT", "TCP stream available after reconnect, sending command");

                match stream.write_all(json_str.as_bytes()).await {
                    Ok(()) => {
                        match stream.flush().await {
                            Ok(()) => {
                                debug!("Command sent successfully after reconnection: {}", json_str);
                                crate::debug_logger::DebugLogger::log_tcp_command_success(&self.config.device_id, &format!("{} (after reconnect)", command_name));

                                // For reset commands, we need to be more careful about the TCP connection state
                                if is_reset_command {
                                    // NOTE: The ESP might not actually receive this command if the TCP connection is stale
                                    // This is why only the first 2 resets work - subsequent reconnects create "zombie" connections
                                    warn!("RESET COMMAND: Reset sent after reconnect - ESP might not receive this due to stale TCP connection!");
                                    crate::debug_logger::DebugLogger::log_reset_success(&self.config.device_id, reset_attempt_number);
                                    // Close TCP stream and set to Connecting state (same as normal reset path)
                                    *tcp = None;
                                    {
                                        let mut state = self.connection_state.write().await;
                                        *state = ConnectionState::Connecting;
                                    }
                                    info!("RESET COMMAND: TCP stream closed after reconnect reset for device {}, connection kept alive for automatic reconnect", self.config.device_id);
                                }

                                Ok(())
                            }
                            Err(e) => {
                                crate::debug_logger::DebugLogger::log_tcp_command_failed(&self.config.device_id, &command_name, &format!("flush failed after reconnect: {}", e));
                                Err(e.into())
                            }
                        }
                    }
                    Err(e) => {
                        crate::debug_logger::DebugLogger::log_tcp_command_failed(&self.config.device_id, &command_name, &format!("write failed after reconnect: {}", e));
                        Err(e.into())
                    }
                }
            } else {
                crate::debug_logger::DebugLogger::log_tcp_connection_status(&self.config.device_id, "STILL_NOT_AVAILABLE", "TCP stream is still None even after reconnect");
                crate::debug_logger::DebugLogger::log_tcp_command_failed(&self.config.device_id, &command_name, "TCP connection still not available after reconnect");
                if is_reset_command {
                    crate::debug_logger::DebugLogger::log_reset_failure(&self.config.device_id, reset_attempt_number, "Failed to reconnect to ESP32");
                }
                Err(Esp32Error::ConnectionFailed("Failed to reconnect to ESP32".to_string()))
            }
        }
    }
    
    // ========================================================================
    // TCP CONNECTION HANDLING
    // ========================================================================
    
    /// Establish TCP connection to ESP32
    async fn connect_tcp(&self) -> Esp32Result<()> {
        let tcp_addr = self.config.tcp_addr();
        debug!("Connecting to TCP address: {}", tcp_addr);

        // Try to connect with timeout
        let stream = timeout(Duration::from_secs(5), TcpStream::connect(tcp_addr))
            .await
            .map_err(|_| Esp32Error::Timeout)?
            .map_err(|e| Esp32Error::ConnectionFailed(format!("TCP connection failed: {}", e)))?;

        // Configure TCP socket for faster disconnect detection
        if let Err(e) = stream.set_nodelay(true) {
            warn!("Failed to set TCP_NODELAY for device {}: {}", self.config.device_id, e);
        }

        // Enable TCP keep-alive with shorter intervals
        let socket2_socket = socket2::Socket::from(stream.into_std()?);

        // Enable keep-alive
        if let Err(e) = socket2_socket.set_keepalive(true) {
            warn!("Failed to enable TCP keep-alive for device {}: {}", self.config.device_id, e);
        }

        // Set TCP keep-alive for 10 minute disconnect detection
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            use socket2::TcpKeepalive;
            let keepalive = TcpKeepalive::new()
                .with_time(Duration::from_secs(600))     // Start after 10 minutes of inactivity
                .with_interval(Duration::from_secs(60)); // Send probe every 60 seconds

            if let Err(e) = socket2_socket.set_tcp_keepalive(&keepalive) {
                warn!("Failed to set TCP keep-alive parameters for device {}: {}", self.config.device_id, e);
            } else {
                info!("TCP keep-alive enabled for device {} (10min idle, 60s interval)", self.config.device_id);
            }
        }

        // Note: Additional Windows TCP optimizations would require more complex winapi setup

        // Note: SO_LINGER removed - it was causing connection issues

        // Convert back to tokio TcpStream
        let stream = TcpStream::from_std(socket2_socket.into())?;
        
        // Store stream
        {
            let mut tcp = self.tcp_stream.lock().await;
            *tcp = Some(stream);
        }
        
        // Update connection state
        {
            let mut state = self.connection_state.write().await;
            *state = ConnectionState::Connected;
        }
        
        debug!("TCP connection established to {}", tcp_addr);
        Ok(())
    }
    
    /// Start background task for TCP message handling
    async fn start_tcp_listener_task(&self, mut shutdown_rx: mpsc::UnboundedReceiver<()>) {
        let tcp_stream = Arc::clone(&self.tcp_stream);
        let tcp_buffer = Arc::clone(&self.tcp_buffer);
        let _event_sender = self.event_sender.clone();
        let _connection_state = Arc::clone(&self.connection_state);
        let device_id = self.config.device_id.clone();
        let _device_config = self.config.clone();
        let device_store = self.device_store.clone();
        let unified_connection_states = Arc::clone(&self.unified_connection_states);
        let device_connection_types = Arc::clone(&self.device_connection_types);

        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            info!("TCP LISTENER TASK: Started for device {} (using TCP keep-alive only)", device_id);

            loop {
                // Check for shutdown signal
                if shutdown_rx.try_recv().is_ok() {
                    debug!("TCP listener task shutting down for device {}", device_id);
                    break;
                }

                // Read from TCP stream for incoming messages from ESP32
                let mut tcp = tcp_stream.lock().await;
                if let Some(stream) = tcp.as_mut() {
                    // Try to read from TCP stream with timeout
                    let read_result = tokio::time::timeout(
                        Duration::from_millis(100),
                        stream.read(&mut buffer)
                    ).await;

                    match read_result {
                        Ok(Ok(0)) => {
                            // Connection closed
                            info!("TCP connection closed for device {}", device_id);
                            *tcp = None;
                        }
                        Ok(Ok(bytes_read)) => {
                            // Got data from ESP32
                            let message = String::from_utf8_lossy(&buffer[..bytes_read]);
                            info!("TCP RECEIVED from {}: {}", device_id, message);
                            crate::debug_logger::DebugLogger::log_tcp_message(&device_id, "RECEIVED", &message);

                            // Add to TCP buffer for processing
                            {
                                let mut buffer_guard = tcp_buffer.lock().await;
                                buffer_guard.push_str(&message);
                            }

                            // Process complete JSON messages from buffer
                            let mut buffer_guard = tcp_buffer.lock().await;
                            while let Some(json_str) = extract_complete_json(&mut buffer_guard) {
                                info!("TCP JSON extracted: {}", json_str);
                                // Process the TCP message using direct DeviceStore bypass
                                let device_id_clone = device_id.clone();
                                let json_clone = json_str.clone();
                                let device_store_clone = device_store.clone();
                                let unified_connection_states_clone = Arc::clone(&unified_connection_states);
                                let device_connection_types_clone = Arc::clone(&device_connection_types);
                                tokio::spawn(async move {
                                    crate::esp32_manager::Esp32Manager::handle_tcp_message_bypass(
                                        &json_clone,
                                        &device_id_clone,
                                        &device_store_clone,
                                        &unified_connection_states_clone,
                                        &device_connection_types_clone
                                    ).await;
                                });
                            }
                        }
                        Ok(Err(e)) => {
                            // Read error
                            warn!("TCP read error for device {}: {}", device_id, e);
                            sleep(Duration::from_millis(100)).await;
                        }
                        Err(_) => {
                            // Timeout - no data available, continue loop
                        }
                    }
                    drop(tcp);
                } else {
                    // No connection, wait a bit
                    sleep(Duration::from_millis(100)).await;
                }
            }

            debug!("TCP listener task ended for device {}", device_id);
        });
    }
    
    // ========================================================================
    // UTILITY METHODS
    // ========================================================================
}

// ============================================================================
// MESSAGE PARSING HELPERS
// ============================================================================

/// Extract complete JSON object from TCP buffer
fn extract_complete_json(buffer: &mut String) -> Option<String> {
    let text = buffer.trim_start();
    if text.is_empty() {
        return None;
    }
    
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, c) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        match c {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => bracket_count += 1,
            '}' if !in_string => {
                bracket_count -= 1;
                if bracket_count == 0 {
                    // Found complete JSON
                    let json_str = text[..=i].to_string();
                    *buffer = text[i + 1..].to_string();
                    return Some(json_str);
                }
            }
            _ => {}
        }
    }
    
    None
}




impl Drop for Esp32Connection {
    fn drop(&mut self) {
        error!("ESP32CONNECTION DROP DEBUG: ESP32Connection for device {} is being DROPPED! This will close the event_sender!", self.config.device_id);
        crate::debug_logger::DebugLogger::log_event("ESP32_CONNECTION", &format!("CONNECTION_DROPPED: {}", self.config.device_id));
        crate::debug_logger::DebugLogger::log_connection_drop(&self.config.device_id, "ESP32Connection struct dropped");

        // Send shutdown signal if we have one
        if let Some(shutdown_tx) = &self.shutdown_sender {
            let _ = shutdown_tx.send(());
        }
    }
}