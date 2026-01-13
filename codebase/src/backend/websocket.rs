// ============================================================================
// WEBSOCKET HANDLER - WebSocket Communication for ESP32 Device Management
// ============================================================================

use crate::auth::{validate_jwt, Claims};
use crate::device_store::{SharedDeviceStore};
use crate::events::{ClientMessage, ServerMessage, DeviceEvent};
use crate::database::DatabaseManager;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, ConnectInfo,
    },
    response::Response,
    http::StatusCode,
};
use axum_extra::extract::CookieJar;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use futures::{sink::SinkExt, stream::StreamExt};
use tracing::{info, warn, error, debug};

// ============================================================================
// APPLICATION STATE FOR WEBSOCKET
// ============================================================================

#[derive(Clone)]
pub struct WebSocketState {
    pub device_store: SharedDeviceStore,
    pub db: Arc<DatabaseManager>,
    pub esp32_manager: Arc<crate::esp32_manager::Esp32Manager>,
    pub esp32_discovery: Arc<tokio::sync::Mutex<crate::esp32_discovery::Esp32Discovery>>,
    pub uart_connection: Arc<tokio::sync::Mutex<crate::uart_connection::UartConnection>>,
}

// ============================================================================
// WEBSOCKET UPGRADE HANDLER
// ============================================================================

/// WebSocket upgrade handler with optional JWT authentication
/// Route: GET /channel/
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebSocketState>,
    cookie_jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Response, (StatusCode, String)> {
    info!("🔥 WebSocket handler called from {}", addr);
    
    // Check if this is a proper WebSocket upgrade request
    info!("Headers: Connection upgrade request");
    
    // JWT Token authentication for WebSocket (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    // Validate JWT token (optional)
    let claims = match token {
        Some(token_value) => {
            match crate::auth::validate_jwt(token_value) {
                Ok(claims) => {
                    info!("WebSocket authenticated user: {} ({})", claims.display_name, claims.email);
                    Some(claims)
                },
                Err(e) => {
                    warn!("WebSocket: Invalid JWT token: {:?}, continuing as guest", e);
                    None
                }
            }
        }
        None => {
            info!("WebSocket: No auth token found, continuing as guest");
            None
        }
    };
    
    // Generate unique client ID for this connection
    let client_id = match &claims {
        Some(c) => generate_client_id(&c.email),
        None => generate_client_id("guest"),
    };
    
    // Upgrade to WebSocket connection
    let response = ws.on_upgrade(move |socket| {
        handle_websocket_connection(socket, state, claims, client_id, addr)
    });
    
    Ok(response)
}

// ============================================================================
// WEBSOCKET CONNECTION HANDLING
// ============================================================================

/// Handle an individual WebSocket connection
async fn handle_websocket_connection(
    socket: WebSocket,
    state: WebSocketState,
    jwt_claims: Option<Claims>,
    client_id: String,
    addr: SocketAddr,
) {
    let user_info = match &jwt_claims {
        Some(claims) => format!("{} ({})", claims.email, claims.display_name),
        None => "guest user".to_string(),
    };
    info!("WebSocket connection established for client {} (user: {}, addr: {})", 
          client_id, user_info, addr);
    
    let user_id = match &jwt_claims {
        Some(claims) => claims.user_id.clone(),
        None => "guest".to_string(),
    };
    let display_name = match &jwt_claims {
        Some(claims) => claims.display_name.clone(),
        None => "Guest User".to_string(),
    };
    let (mut sender, mut receiver) = socket.split();
    
    // Create channel for sending messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
    
    // Clone client_id for the outgoing task
    let client_id_for_task = client_id.clone();
    
    // Spawn task to handle outgoing messages
    let outgoing_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(json) => {
                    if let Err(e) = sender.send(Message::Text(json)).await {
                        error!("Failed to send WebSocket message: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                }
            }
        }
        debug!("Outgoing message task ended for client {}", client_id_for_task);
    });
    
    // Handle incoming messages
    let device_store = state.device_store.clone();
    let db = state.db.clone();
    let mut registered_devices: Vec<String> = Vec::new();
    
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                info!("WebSocket message received from client {}: {}", client_id, text);
                match handle_client_message(
                    &text,
                    &device_store,
                    &db,
                    &state.esp32_manager,
                    &state.esp32_discovery,
                    &state.uart_connection,
                    &user_id,
                    &display_name,
                    &client_id,
                    &tx,
                    &mut registered_devices
                ).await {
                    Ok(()) => {
                        debug!("Processed message from client {}: {}", client_id, text);
                    }
                    Err(e) => {
                        error!("Error processing message from client {}: {}", client_id, e);
                        // Send error response back to client
                        let error_response = ServerMessage::device_events(
                            "error".to_string(),
                            vec![]
                        );
                        if let Err(send_err) = tx.send(error_response) {
                            error!("Failed to send error response: {}", send_err);
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection closed by client {}", client_id);
                break;
            }
            Ok(Message::Ping(_data)) => {
                debug!("Received ping from client {}", client_id);
                // Pong will be sent automatically by axum
            }
            Ok(Message::Pong(_)) => {
                debug!("Received pong from client {}", client_id);
            }
            Ok(Message::Binary(_)) => {
                warn!("Received unexpected binary message from client {}", client_id);
            }
            Err(e) => {
                error!("WebSocket error for client {}: {}", client_id, e);
                break;
            }
        }
    }
    
    // Cleanup: unregister from all devices
    for device_id in registered_devices {
        if let Err(e) = device_store.unregister_client(&device_id, &client_id).await {
            error!("Failed to unregister client {} from device {}: {}", client_id, device_id, e);
        }
    }
    
    // Cancel outgoing task
    outgoing_task.abort();
    
    info!("WebSocket connection terminated for client {} (user: {})", client_id, user_id);
}

// ============================================================================
// MESSAGE HANDLING
// ============================================================================

/// Handle incoming client message
async fn handle_client_message(
    message_text: &str,
    device_store: &SharedDeviceStore,
    db: &Arc<DatabaseManager>,
    esp32_manager: &Arc<crate::esp32_manager::Esp32Manager>,
    esp32_discovery: &Arc<tokio::sync::Mutex<crate::esp32_discovery::Esp32Discovery>>,
    uart_connection: &Arc<tokio::sync::Mutex<crate::uart_connection::UartConnection>>,
    user_id: &str,
    display_name: &str,
    client_id: &str,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    registered_devices: &mut Vec<String>,
) -> Result<(), String> {
    // First, try to parse as a generic JSON to check for heartbeat messages
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(message_text) {
        if let Some(msg_type) = json_value.get("type").and_then(|t| t.as_str()) {
            if msg_type == "ping" {
                // Handle heartbeat ping - send pong response
                debug!("Received ping from client {}, sending pong", client_id);
                
                // Extract timestamp from ping message if present
                let timestamp = json_value.get("timestamp")
                    .and_then(|t| t.as_u64());
                
                // Send pong response using existing message channel
                let pong_response = ServerMessage::pong(timestamp);
                tx.send(pong_response)
                    .map_err(|e| format!("Failed to send pong response: {}", e))?;
                
                debug!("Sent pong response to client {}", client_id);
                return Ok(());
            }
        }
    }
    
    // Parse as ClientMessage for actual canvas operations
    info!("Parsing ClientMessage JSON: {}", message_text);
    let client_message: ClientMessage = serde_json::from_str(message_text)
        .map_err(|e| {
            error!("Failed to parse ClientMessage JSON: {}", e);
            error!("Raw JSON: {}", message_text);
            format!("Invalid ClientMessage JSON: {}", e)
        })?;

    info!("Successfully parsed ClientMessage: {:?}", client_message);
    
    match client_message {
        ClientMessage::RegisterForDevice { device_id, subscription_type } => {
            info!("Processing RegisterForDevice request for device_id: {} with subscription: {:?}", device_id, subscription_type);
            handle_register_for_device(
                device_id,
                device_store,
                esp32_manager,
                esp32_discovery,
                uart_connection,
                db,
                user_id,
                display_name,
                client_id,
                tx,
                registered_devices,
                subscription_type,
            ).await
        }
        
        ClientMessage::UnregisterForDevice { device_id } => {
            handle_unregister_for_device(
                device_id,
                device_store,
                client_id,
                registered_devices
            ).await
        }
        
        ClientMessage::DeviceEvent { device_id, events_for_device } => {
            handle_device_events(
                device_id,
                events_for_device,
                device_store,
                db,
                esp32_manager,
                uart_connection,
                user_id,
                client_id,
                registered_devices
            ).await
        }
    }
}

/// Handle registerForDevice command
async fn handle_register_for_device(
    device_id: String,
    device_store: &SharedDeviceStore,
    esp32_manager: &Arc<crate::esp32_manager::Esp32Manager>,
    esp32_discovery: &Arc<tokio::sync::Mutex<crate::esp32_discovery::Esp32Discovery>>,
    uart_connection: &Arc<tokio::sync::Mutex<crate::uart_connection::UartConnection>>,
    db: &Arc<DatabaseManager>,
    user_id: &str,
    display_name: &str,
    client_id: &str,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    registered_devices: &mut Vec<String>,
    subscription_type: crate::events::SubscriptionType,
) -> Result<(), String> {
    info!("handle_register_for_device called - device_id: {}, user_id: {}, client_id: {}", device_id, user_id, client_id);
    // Check if user has permission to access this device (requires at least Read permission)
    // Allow access to "system" device for all authenticated users (for ESP32 discovery)
    // Also allow access to discovered ESP32 devices (identified by device_id starting with "esp32-" or MAC address format)
    let has_permission = if user_id == "guest" {
        true  // TEMPORARY: Allow guest user to access all devices
    } else if device_id == "system" {
        true  // Allow all authenticated users to access system events
    } else if device_id.starts_with("esp32-") {
        true  // Allow all authenticated users to access discovered ESP32 devices
    } else if is_mac_address_format(&device_id) || is_mac_key_format(&device_id) {
        true  // Allow all authenticated users to access ESP32 devices identified by MAC address
    } else if is_stm32_uid_format(&device_id) {
        true  // Allow all authenticated users to access STM32 devices identified by UID (24 hex chars)
    } else {
        db.user_has_device_permission(&device_id, user_id, "R").await
            .map_err(|e| format!("Database error checking permissions: {}", e))?
    };
    
    if !has_permission {
        return Err(format!("User {} does not have permission to access device {}", user_id, device_id));
    }
    
    info!("User {} has access permission for device {}", user_id, device_id);
    
    info!("Registering client {} for device {} (user: {}) with subscription: {:?}", client_id, device_id, user_id, subscription_type);

    // Register client and get existing events for replay
    let existing_events = device_store.register_client(
        device_id.clone(),
        user_id.to_string(),
        display_name.to_string(),
        client_id.to_string(),
        tx.clone(),
        subscription_type.clone(),
    ).await?;
    
    // Add to registered devices list
    if !registered_devices.contains(&device_id) {
        registered_devices.push(device_id.clone());
    }

    // Check device type from registry (or infer from format if not yet registered)
    // Only connect for FULL subscriptions - light subscriptions just need status
    let device_type = esp32_manager.get_device_connection_type(&device_id).await;
    let is_uart_device = device_type == Some(crate::esp32_manager::DeviceConnectionType::Uart);
    let is_tcp_udp_device = device_type == Some(crate::esp32_manager::DeviceConnectionType::TcpUdp);

    // For devices not yet in registry, infer from format (MAC addresses are TCP/UDP)
    let inferred_tcp_udp = device_type.is_none() && (is_mac_address_format(&device_id) || is_mac_key_format(&device_id));
    let is_esp32_tcp_device = is_tcp_udp_device || inferred_tcp_udp;

    if is_esp32_tcp_device && subscription_type == crate::events::SubscriptionType::Full {
        info!("Attempting to add and connect TCP/UDP ESP32 device: {} (full subscription)", device_id);

        // First check if device is already added to manager
        let device_exists = esp32_manager.get_device_config(&device_id).await.is_some();

        if !device_exists {
            // Try to get device configuration from discovery data
            info!("ESP32 device {} not in manager, trying to find it in discovery data", device_id);

            // Look up the device in discovery data
            let discovery_config = {
                let discovery = esp32_discovery.lock().await;
                let discovered_devices = discovery.get_discovered_devices().await;
                discovered_devices.get(&device_id).map(|d| d.device_config.clone())
            };

            let config = match discovery_config {
                Some(discovered_config) => {
                    info!("Found ESP32 device {} in discovery data: {}:{}",
                          device_id, discovered_config.ip_address, discovered_config.tcp_port);
                    discovered_config
                }
                None => {
                    info!("ESP32 device {} not found in discovery data, using default config", device_id);
                    // Fallback to default configuration
                    crate::esp32_types::Esp32DeviceConfig::new(
                        device_id.clone(),
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100)), // Default fallback
                        3232, // ESP32 TCP port
                        3232, // ESP32 UDP port
                    )
                }
            };

            // Add the device to the manager
            match esp32_manager.add_device(config).await {
                Ok(()) => {
                    info!("Successfully added ESP32 device {} to manager", device_id);
                }
                Err(e) => {
                    warn!("Failed to add ESP32 device {} to manager: {}", device_id, e);
                    // Don't return error here - the device might still work if manually configured
                }
            }
        } else {
            info!("ESP32 device {} already exists in manager", device_id);
        }

        // Now try to connect the device
        match esp32_manager.connect_device(&device_id).await {
            Ok(()) => {
                info!("Successfully connected ESP32 device: {}", device_id);
            }
            Err(e) => {
                warn!("Failed to connect ESP32 device {}: {}. Device will show as disconnected until manually connected.", device_id, e);
                // Don't fail the registration - user should still be able to see the device
            }
        }
    } else if is_uart_device && subscription_type == crate::events::SubscriptionType::Full {
        info!("Full subscription for UART device: {} - device is already connected via UART", device_id);
        // UART devices are always connected if UART connection is active

        // Send tab opened notification to UART device
        let tab_opened_message = serde_json::json!({
            "device_id": device_id,
            "event": "tabOpened"
        });

        let message_str = serde_json::to_string(&tab_opened_message)
            .map_err(|e| format!("Failed to serialize tab opened message: {}", e))?;

        let uart_conn = uart_connection.lock().await;
        if let Err(e) = uart_conn.send_command(&device_id, &message_str).await {
            warn!("Failed to send tab opened notification to UART device {}: {}", device_id, e);
            // Don't fail the registration - user should still be able to see the device
        } else {
            info!("Tab opened notification sent to UART device {}", device_id);
        }
    } else if is_esp32_tcp_device {
        info!("Light subscription for TCP/UDP ESP32 device {} - will add to manager but not connect", device_id);

        // For light subscriptions, add device to manager (if not exists) but don't connect
        let device_exists = esp32_manager.get_device_config(&device_id).await.is_some();

        if !device_exists {
            // Try to get device configuration from discovery data
            info!("ESP32 device {} not in manager, trying to find it in discovery data for light subscription", device_id);

            // Look up the device in discovery data
            let discovery_config = {
                let discovery = esp32_discovery.lock().await;
                let discovered_devices = discovery.get_discovered_devices().await;
                discovered_devices.get(&device_id).map(|d| d.device_config.clone())
            };

            let config = match discovery_config {
                Some(discovered_config) => {
                    info!("Found ESP32 device {} in discovery data: {}:{}",
                          device_id, discovered_config.ip_address, discovered_config.tcp_port);
                    discovered_config
                }
                None => {
                    info!("ESP32 device {} not found in discovery data, using default config", device_id);
                    // Fallback to default configuration
                    crate::esp32_types::Esp32DeviceConfig::new(
                        device_id.clone(),
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100)), // Default fallback
                        3232, // ESP32 TCP port
                        3232, // ESP32 UDP port
                    )
                }
            };

            // Add the device to the manager (but don't connect it)
            match esp32_manager.add_device(config.clone()).await {
                Ok(()) => {
                    info!("Successfully added ESP32 device {} to manager for light subscription", device_id);

                    // Send initial disconnected status for newly added device
                    let status_event = crate::events::DeviceEvent::esp32_connection_status(
                        device_id.clone(),
                        false, // Not connected yet
                        config.ip_address.to_string(),
                        config.tcp_port,
                        config.udp_port
                    );

                    let status_response = ServerMessage::device_events(
                        device_id.clone(),
                        vec![status_event]
                    );

                    if let Err(e) = tx.send(status_response) {
                        warn!("Failed to send initial disconnected status: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to add ESP32 device {} to manager: {}", device_id, e);
                }
            }
        } else {
            // Device already exists, get and send current connection status
            info!("ESP32 device {} already exists in manager, sending current status", device_id);

            if let Some(config) = esp32_manager.get_device_config(&device_id).await {
                if let Some(state) = esp32_manager.get_device_state(&device_id).await {
                    let is_connected = state.is_connected();
                    info!("Sending initial connection status for light subscription: device {} is {}",
                          device_id, if is_connected { "connected" } else { "disconnected" });

                    let status_event = crate::events::DeviceEvent::esp32_connection_status(
                        device_id.clone(),
                        is_connected,
                        config.ip_address.to_string(),
                        config.tcp_port,
                        config.udp_port
                    );

                    let status_response = ServerMessage::device_events(
                        device_id.clone(),
                        vec![status_event]
                    );

                    if let Err(e) = tx.send(status_response) {
                        warn!("Failed to send initial connection status for light subscription: {}", e);
                    }
                }
            }
        }
    }

    // Send existing events to client for replay
    if !existing_events.is_empty() {
        let event_count = existing_events.len();
        let response = ServerMessage::device_events(
            device_id.clone(),
            existing_events
        );
        
        tx.send(response)
            .map_err(|e| format!("Failed to send events to client: {}", e))?;
        
        info!("Sent {} existing events to client {} for device {}", 
              event_count, client_id, device_id);
    } else {
        // Send empty events list to confirm successful registration
        let response = ServerMessage::device_events(
            device_id.clone(),
            vec![]
        );

        tx.send(response)
            .map_err(|e| format!("Failed to send registration confirmation to client: {}", e))?;

        info!("Sent registration confirmation to client {} for device {} (no existing events)",
              client_id, device_id);
        info!("FRONTEND DEBUG: Client {} registered for device {} - frontend should now receive device events to update connection status", client_id, device_id);
    }

    Ok(())
}

/// Handle unregisterForDevice command
async fn handle_unregister_for_device(
    device_id: String,
    device_store: &SharedDeviceStore,
    client_id: &str,
    registered_devices: &mut Vec<String>,
) -> Result<(), String> {
    info!("Unregistering client {} from device {}", client_id, device_id);
    
    // Unregister from device store
    device_store.unregister_client(&device_id, client_id).await?;
    
    // Remove from registered devices list
    registered_devices.retain(|id| id != &device_id);
    
    Ok(())
}

/// Handle device events from client
async fn handle_device_events(
    device_id: String,
    events: Vec<DeviceEvent>,
    device_store: &SharedDeviceStore,
    db: &Arc<DatabaseManager>,
    esp32_manager: &Arc<crate::esp32_manager::Esp32Manager>,
    uart_connection: &Arc<tokio::sync::Mutex<crate::uart_connection::UartConnection>>,
    user_id: &str,
    client_id: &str,
    registered_devices: &[String],
) -> Result<(), String> {
    info!("DEVICE EVENTS DEBUG: handle_device_events called for device {} by client {}, registered_devices: {:?}", device_id, client_id, registered_devices);

    // Check if client is registered for this device
    if !registered_devices.contains(&device_id) {
        error!("DEVICE EVENTS DEBUG: Client {} is not registered for device {} - current registered devices: {:?}", client_id, device_id, registered_devices);
        return Err(format!("Client {} is not registered for device {}", client_id, device_id));
    }
    
    // Check write permissions for device operations
    // Allow access to ESP32 devices (identified by MAC address format or esp32-XX format for UART) for all users
    let is_esp32_device = is_mac_address_format(&device_id)
        || is_mac_key_format(&device_id)
        || device_id.starts_with("esp32-");  // UART devices use esp32-XX format

    let has_write_permission = if user_id == "guest" {
        true  // TEMPORARY: Allow guest user to write to all devices
    } else if is_esp32_device {
        true  // Allow all users to control ESP32 devices
    } else if is_stm32_uid_format(&device_id) {
        true  // Allow all users to control STM32 devices identified by UID
    } else {
        db.user_has_device_permission(&device_id, user_id, "W").await
            .map_err(|e| format!("Database error checking write permissions: {}", e))?
    };

    if !has_write_permission {
        return Err(format!("User {} does not have write permission for device {}", user_id, device_id));
    }
    
    info!("User {} has write permission for device {}", user_id, device_id);
    
    // Process each event
    for event in events {
        debug!("Processing event from client {} for device {}: {:?}", client_id, device_id, event);

        // Check if this is an ESP32 command event
        if let DeviceEvent::Esp32Command { command, .. } = &event {
            // Route command based on device type from registry
            let device_type = esp32_manager.get_device_connection_type(&device_id).await;
            let is_uart = device_type == Some(crate::esp32_manager::DeviceConnectionType::Uart);

            if is_uart {
                // UART device - route to UART connection
                let command_json = serde_json::to_string(&command)
                    .map_err(|e| format!("Failed to serialize command: {}", e))?;

                let uart_conn = uart_connection.lock().await;
                if let Err(e) = uart_conn.send_command(&device_id, &command_json).await {
                    error!("Failed to send UART command to device {}: {}", device_id, e);
                    return Err(format!("UART command failed: {}", e));
                }
            } else {
                // TCP/UDP device - route to ESP32 manager

                if let Err(e) = esp32_manager.handle_websocket_command(
                    &device_id,
                    command.clone(),
                    user_id,
                    client_id,
                ).await {
                    error!("Failed to handle ESP32 command for device {}: {}", device_id, e);
                    return Err(format!("ESP32 command failed: {}", e));
                }

                debug!("ESP32 command processed successfully for device {}", device_id);
            }

            continue; // Command handlers handle the event broadcasting
        }

        // Add event to store (this will also broadcast to other clients)
        device_store.add_event(device_id.clone(), event, user_id.to_string(), client_id.to_string()).await?;
    }
    
    Ok(())
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Extract and validate JWT from HTTP cookies
async fn extract_jwt_from_cookies(cookie_jar: &CookieJar) -> Result<Claims, String> {
    // Get auth token from cookie
    let token = cookie_jar.get("auth_token")
        .ok_or("No auth token found in cookies")?
        .value();
    
    // Validate JWT
    validate_jwt(token)
        .map_err(|e| format!("Invalid JWT: {}", e))
}

/// Generate a unique client ID based on user email with UUID for multi-tab support
/// This creates a unique ID per browser tab/connection while maintaining user consistency
fn generate_client_id(email: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    email.hash(&mut hasher);
    let user_hash = hasher.finish();

    // Add UUID for unique tab identification while keeping user hash for consistency
    let unique_id = uuid::Uuid::new_v4().to_string()[..8].to_string(); // Short UUID

    format!("client-{:x}-{}", user_hash, unique_id)
}

/// Check if a device_id is in MAC address format (XX:XX:XX:XX:XX:XX)
/// Used to identify discovered ESP32 devices that use MAC address as device_id
fn is_mac_address_format(device_id: &str) -> bool {
    // Check if it matches MAC address pattern: XX:XX:XX:XX:XX:XX
    // where X is a hexadecimal digit
    if device_id.len() != 17 {
        return false;
    }

    let parts: Vec<&str> = device_id.split(':').collect();
    if parts.len() != 6 {
        return false;
    }

    // Check each part is exactly 2 hex digits
    for part in parts {
        if part.len() != 2 {
            return false;
        }
        if !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }

    true
}


/// Check if a device_id is in MAC key format (XX-XX-XX-XX-XX-XX)
/// Used to identify ESP32 devices that use MAC address with dashes as device_id
fn is_mac_key_format(device_id: &str) -> bool {
    // Check if it matches MAC key pattern: XX-XX-XX-XX-XX-XX
    // where X is a hexadecimal digit
    if device_id.len() != 17 {
        return false;
    }

    let parts: Vec<&str> = device_id.split('-').collect();
    if parts.len() != 6 {
        return false;
    }

    // Check each part is exactly 2 hex digits
    for part in parts {
        if part.len() != 2 {
            return false;
        }
        if !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }

    true
}

/// Check if a device_id is an STM32 UID format (24 hexadecimal characters)
/// STM32 UIDs are 96-bit unique identifiers represented as 24 hex chars
fn is_stm32_uid_format(device_id: &str) -> bool {
    device_id.len() == 24 && device_id.chars().all(|c| c.is_ascii_hexdigit())
}

// ============================================================================
// WEBSOCKET STATISTICS ENDPOINT
// ============================================================================

/// Get WebSocket statistics (for monitoring/debugging)
pub async fn websocket_stats_handler(
    State(state): State<WebSocketState>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    let stats = state.device_store.get_stats().await;
    let active_devices = state.device_store.get_active_devices().await;
    
    Ok(axum::Json(serde_json::json!({
        "websocket_stats": {
            "total_devices": stats.total_devices,
            "total_events": stats.total_events,
            "active_devices": stats.active_devices,
            "total_connections": stats.total_connections,
            "average_events_per_device": stats.average_events_per_device,
            "average_connections_per_device": stats.average_connections_per_device,
            "active_device_details": active_devices
        }
    })))
}

/// Get users currently connected to a device
pub async fn device_users_handler(
    axum::extract::Path(device_id): axum::extract::Path<String>,
    State(state): State<WebSocketState>,
    cookie_jar: CookieJar,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    // Authenticate user
    let _claims = match extract_jwt_from_cookies(&cookie_jar).await {
        Ok(claims) => claims,
        Err(_) => return Err(axum::http::StatusCode::UNAUTHORIZED),
    };
    
    // Get users for device with database lookup for display names
    let users = state.device_store.get_device_users_with_db(&device_id, &state.db).await;
    
    Ok(axum::Json(serde_json::json!({
        "device_id": device_id,
        "users": users
    })))
}

// ============================================================================
// WEBSOCKET CLEANUP TASK
// ============================================================================

/// Background task to clean up stale WebSocket connections
pub async fn start_cleanup_task(device_store: SharedDeviceStore) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    
    loop {
        interval.tick().await;
        
        match device_store.cleanup_stale_connections().await {
            count if count > 0 => info!("Cleaned up {} stale WebSocket connections", count),
            _ => debug!("No stale connections to clean up"),
        }
    }
}