// ESP32 device event store for multiuser functionality

use crate::events::{DeviceEvent, EventWithMetadata, ServerMessage};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, warn, error, debug};

// User color generation system
const USER_COLORS: &[&str] = &[
    "#FF6B6B", // Red
    "#4ECDC4", // Teal
    "#45B7D1", // Blue
    "#96CEB4", // Green
    "#FFEAA7", // Yellow
    "#DDA0DD", // Plum
    "#98D8C8", // Mint
    "#F7DC6F", // Lemon
    "#BB8FCE", // Lavender
    "#85C1E9", // Sky Blue
    "#F8C471", // Orange
    "#82E0AA", // Light Green
    "#F1948A", // Salmon
    "#AED6F1", // Light Blue
    "#A9DFBF", // Pale Green
    "#F9E79F", // Pale Yellow
];

/// Generate a user color based on user_id (deterministic but well-distributed)
fn generate_user_color(user_id: &str, existing_colors: &[String]) -> String {
    // Use a better hash function based on FNV-1a algorithm for better distribution
    let hash = fnv_hash(user_id);
    let primary_index = (hash as usize) % USER_COLORS.len();
    let preferred_color = USER_COLORS[primary_index].to_string();
    
    debug!("Hash {} -> index {} -> color {} for user_id '{}'", 
           hash, primary_index, preferred_color, user_id);
    
    // If preferred color is not taken, use it
    if !existing_colors.contains(&preferred_color) {
        debug!("Preferred color {} available for user {}", preferred_color, user_id);
        return preferred_color;
    }
    
    debug!("Preferred color {} taken, finding alternative for user {}", preferred_color, user_id);
    
    // Use deterministic fallback: try colors in hash-based order, not sequential
    for i in 1..USER_COLORS.len() {
        let fallback_index = (primary_index + i) % USER_COLORS.len();
        let fallback_color = USER_COLORS[fallback_index].to_string();
        
        if !existing_colors.contains(&fallback_color) {
            debug!("Found alternative color {} (index {}) for user {}", 
                   fallback_color, fallback_index, user_id);
            return fallback_color;
        }
    }
    
    // Ultimate fallback: if all 16 colors are taken, create slight variation
    warn!("All {} colors taken! Creating variation for user {}", USER_COLORS.len(), user_id);
    create_color_variation(&preferred_color, existing_colors.len())
}

/// FNV-1a hash function for better distribution than simple polynomial hash
fn fnv_hash(input: &str) -> u32 {
    const FNV_OFFSET_BASIS: u32 = 2166136261;
    const FNV_PRIME: u32 = 16777619;
    
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Create a slight color variation when all base colors are taken
fn create_color_variation(base_color: &str, variation_factor: usize) -> String {
    // Parse hex color
    if let Ok(color_num) = u32::from_str_radix(&base_color[1..], 16) {
        let r = (color_num >> 16) & 0xFF;
        let g = (color_num >> 8) & 0xFF;
        let b = color_num & 0xFF;
        
        // Apply slight modification based on variation factor
        let mod_factor = (variation_factor % 8) as u32 * 8; // 0, 8, 16, 24, 32, 40, 48, 56
        let r_mod = ((r + mod_factor) % 256).min(255);
        let g_mod = ((g + mod_factor) % 256).min(255); 
        let b_mod = ((b + mod_factor) % 256).min(255);
        
        let modified_color = format!("#{:02X}{:02X}{:02X}", r_mod, g_mod, b_mod);
        debug!("Created color variation: {} -> {} (factor: {})", base_color, modified_color, mod_factor);
        modified_color
    } else {
        // Fallback to original color if parsing fails
        base_color.to_string()
    }
}

// WebSocket client connection management

// Active WebSocket connection to a canvas
#[derive(Debug, Clone)]
pub struct ClientConnection {
    pub user_id: String,
    pub display_name: String,
    pub client_id: String,
    pub user_color: String,
    pub sender: mpsc::UnboundedSender<ServerMessage>,
    pub subscription_type: crate::events::SubscriptionType,
}

impl ClientConnection {
    pub fn new(
        user_id: String,
        display_name: String,
        client_id: String,
        _device_id: String,
        user_color: String,
        sender: mpsc::UnboundedSender<ServerMessage>,
        subscription_type: crate::events::SubscriptionType,
    ) -> Self {
        Self {
            user_id,
            display_name,
            client_id,
            user_color,
            sender,
            subscription_type,
        }
    }
    
    // Send a message to this client
    pub fn send_message(&self, message: ServerMessage) -> Result<(), String> {
        self.sender.send(message)
            .map_err(|e| format!("Failed to send message to client {}: {}", self.client_id, e))
    }
}

// Thread-safe in-memory store for device events and active connections
#[derive(Debug)]
pub struct DeviceEventStore {
    // Events stored per device ID
    device_events: RwLock<HashMap<String, Vec<EventWithMetadata>>>,
    // Active client connections per device ID
    active_connections: RwLock<HashMap<String, Vec<ClientConnection>>>,
    // Debug message limit per device (configurable)
    max_debug_messages_per_device: RwLock<usize>,
}

impl DeviceEventStore {
    // Create a new empty event store
    pub fn new() -> Self {
        Self {
            device_events: RwLock::new(HashMap::new()),
            active_connections: RwLock::new(HashMap::new()),
            max_debug_messages_per_device: RwLock::new(200), // Default: 200
        }
    }

    /// Update the maximum number of debug messages per device
    pub async fn set_max_debug_messages(&self, max: usize) {
        let mut limit = self.max_debug_messages_per_device.write().await;
        *limit = max;
        info!("Debug message limit updated to {} messages per device", max);
    }

    /// Get the current debug message limit
    pub async fn get_max_debug_messages(&self) -> usize {
        *self.max_debug_messages_per_device.read().await
    }
    
    // Event management methods
    
    // Add a new event to a device and broadcast to all connected clients
    pub async fn add_event(
        &self,
        device_id: String,
        event: DeviceEvent,
        user_id: String,
        client_id: String
    ) -> Result<(), String> {
        // Validate event before storing
        event.validate().map_err(|e| {
            error!("Event validation failed for device {}: {}", device_id, e);
            format!("Invalid event: {}", e)
        })?;

        // Check if this is a debug message (UdpBroadcast)
        let is_debug_message = matches!(event, crate::events::DeviceEvent::Esp32UdpBroadcast { .. });

        // Read limit BEFORE taking write lock to avoid deadlock
        let max_debug = if is_debug_message {
            *self.max_debug_messages_per_device.read().await
        } else {
            0 // Not used for non-debug messages
        };

        // Create event with metadata
        let event_with_metadata = EventWithMetadata {
            event: event.clone(),
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            user_id: user_id.clone(),
            is_replay: None,
        };

        // Store event with limit enforcement for debug messages
        {
            let mut events = self.device_events.write().await;
            let device_events = events.entry(device_id.clone()).or_insert_with(Vec::new);

            if is_debug_message && max_debug > 0 {
                // Apply limit only to debug messages (skip if limit is 0)

                // Count existing debug messages
                let debug_count = device_events.iter()
                    .filter(|e| matches!(e.event, crate::events::DeviceEvent::Esp32UdpBroadcast { .. }))
                    .count();

                // If limit reached, remove oldest debug message
                if debug_count >= max_debug {
                    // Find and remove the oldest debug message
                    if let Some(oldest_debug_idx) = device_events.iter()
                        .position(|e| matches!(e.event, crate::events::DeviceEvent::Esp32UdpBroadcast { .. }))
                    {
                        device_events.remove(oldest_debug_idx);
                    }
                }
            }

            device_events.push(event_with_metadata);
        }

        // Broadcast to all connected clients (except sender)
        match self.broadcast_event(&device_id, event, &client_id).await {
            Ok(()) => {}
            Err(e) => {
                error!("WebSocket broadcast failed for device {}: {}", device_id, e);
                return Err(e);
            }
        }

        Ok(())
    }
    
    // Get all events for a device (for replay when client connects)
    pub async fn get_device_events(&self, device_id: &str) -> Vec<DeviceEvent> {
        let events = self.device_events.read().await;
        
        match events.get(device_id) {
            Some(device_events) => {
                device_events.iter()
                    .map(|event_meta| event_meta.event.clone())
                    .collect()
            },
            None => {
                debug!("No events found for device: {}", device_id);
                Vec::new()
            }
        }
    }
    
    // Get device-specific information (placeholder for ESP32 device info)
    pub async fn get_device_info(&self, _device_id: &str) -> Vec<DeviceEvent> {
        // For ESP32 devices, we might return device status, sensor data, etc.
        // For now, return empty - this can be extended for device-specific info
        Vec::new()
    }
    
    // Get event count for a device (for debugging/monitoring)
    pub async fn get_event_count(&self, device_id: &str) -> usize {
        let events = self.device_events.read().await;
        events.get(device_id).map(|v| v.len()).unwrap_or(0)
    }
    
    // Clear all events for a device (for testing or device reset)
    pub async fn clear_device_events(&self, device_id: &str) -> Result<(), String> {
        let mut events = self.device_events.write().await;
        if let Some(device_events) = events.get_mut(device_id) {
            device_events.clear();
            info!("Cleared all events for device: {}", device_id);
        }
        Ok(())
    }
    
    // ========================================================================
    // CONNECTION MANAGEMENT
    // ========================================================================
    
    /// Register a new client connection to a device
    pub async fn register_client(
        &self,
        device_id: String,
        user_id: String,
        display_name: String,
        client_id: String,
        sender: mpsc::UnboundedSender<ServerMessage>,
        subscription_type: crate::events::SubscriptionType,
    ) -> Result<Vec<DeviceEvent>, String> {
        // ATOMIC OPERATION: Generate color and add connection in single critical section
        let (user_color, is_reconnection) = {
            let mut connections = self.active_connections.write().await;
            let device_connections = connections.entry(device_id.clone()).or_insert_with(Vec::new);
            
            // Check if this user already has a color (reconnection)
            let existing_user_color = device_connections.iter()
                .find(|conn| conn.user_id == user_id)
                .map(|conn| conn.user_color.clone());
            
            // Only remove connection if it's the exact same client_id (true reconnection)
            // Multi-tab support: different client_ids from same user should coexist
            let before_count = device_connections.len();
            device_connections.retain(|conn| conn.client_id != client_id);
            let after_count = device_connections.len();

            if before_count > after_count {
                info!("SUBSCRIPTION UPDATE: Removed {} old connection(s) for client_id {} on device {}",
                      before_count - after_count, client_id, device_id);
            } else {
                info!("SUBSCRIPTION NEW: No previous connection found for client_id {} on device {}",
                      client_id, device_id);
            }

            // Check if this is a reconnection (user already has a color)
            let is_reconnection = existing_user_color.is_some();
            
            // Generate color only if user is truly new
            let user_color = if let Some(color) = existing_user_color {
                debug!("User {} reconnecting with existing color: {}", user_id, color);
                color
            } else {
                // Collect already assigned colors in this device (per unique user_id)
                let mut user_colors: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for conn in device_connections.iter() {
                    // Only count each user_id once, regardless of how many connections they have
                    user_colors.insert(conn.user_id.clone(), conn.user_color.clone());
                }
                let existing_colors: Vec<String> = user_colors.values().cloned().collect();
                
                debug!("Device {}: Existing colors for {} users: {:?}", 
                       device_id, user_colors.len(), existing_colors);
                
                // Generate color for this new user
                generate_user_color(&user_id, &existing_colors)
            };
            
            info!("Color {} for user {} on device {}", 
                  user_color, user_id, device_id);
            
            // Create and add new connection atomically
            let connection = ClientConnection::new(
                user_id.clone(),
                display_name.clone(),
                client_id.clone(),
                device_id.clone(),
                user_color.clone(),
                sender,
                subscription_type.clone(),
            );
            info!("SUBSCRIPTION REGISTER: Adding connection for client_id {} on device {} with subscription: {:?}",
                  client_id, device_id, subscription_type);
            device_connections.push(connection);
            
            (user_color, is_reconnection)
        };
        
        info!("Client {} registered for device {} (user: {})", client_id, device_id, user_id);
        
        // Broadcast user joined event only for truly new users (not reconnections)
        if !is_reconnection {
            let user_joined_event = crate::events::DeviceEvent::user_joined(user_id.clone(), display_name.clone(), user_color.clone());
            if let Err(e) = self.broadcast_event(&device_id, user_joined_event, &client_id).await {
                error!("Failed to broadcast user joined event: {}", e);
            }
        } else {
            debug!("Skipping userJoined broadcast for reconnecting user: {}", user_id);
            // Multi-Tab Fix: Send refresh signal to update connection counts in other clients
            let refresh_event = crate::events::DeviceEvent::user_joined("USER_COUNT_REFRESH".to_string(), "".to_string(), "".to_string());
            if let Err(e) = self.broadcast_event(&device_id, refresh_event, &client_id).await {
                error!("Failed to broadcast connection count refresh event: {}", e);
            }
        }
        
        // Return all existing events for replay
        let events = self.get_device_events(&device_id).await;
        
        debug!("Sending {} events to newly registered client {}", 
               events.len(), client_id);
        
        Ok(events)
    }
    
    /// Unregister a client from a device
    pub async fn unregister_client(&self, device_id: &str, client_id: &str) -> Result<(), String> {
        let mut connection_to_remove: Option<ClientConnection> = None;
        
        // First, find and remove the connection while keeping track of user info
        {
            let mut connections = self.active_connections.write().await;
            
            if let Some(device_connections) = connections.get_mut(device_id) {
                let initial_count = device_connections.len();
                
                // Find the connection we're about to remove
                if let Some(conn) = device_connections.iter().find(|conn| conn.client_id == client_id) {
                    connection_to_remove = Some(conn.clone());
                }
                
                // Remove the connection
                device_connections.retain(|conn| conn.client_id != client_id);
                
                if device_connections.len() < initial_count {
                    info!("Client {} unregistered from device {}", client_id, device_id);
                } else {
                    warn!("Attempted to unregister non-existent client {} from device {}", client_id, device_id);
                }
                
                // Clean up empty device entries
                if device_connections.is_empty() {
                    connections.remove(device_id);
                    debug!("Removed empty device connection list for: {}", device_id);
                }
            }
        }
        
        // Broadcast user left event if we found the connection
        if let Some(removed_connection) = connection_to_remove {
            // Check if this user still has other connections to this device
            let user_still_connected = {
                let connections = self.active_connections.read().await;
                if let Some(device_connections) = connections.get(device_id) {
                    device_connections.iter().any(|conn| conn.user_id == removed_connection.user_id)
                } else {
                    false
                }
            };
            
            // Only broadcast user left event if they have no more connections to this device
            if !user_still_connected {
                let user_left_event = crate::events::DeviceEvent::user_left(
                    removed_connection.user_id,
                    removed_connection.display_name,
                    removed_connection.user_color
                );
                if let Err(e) = self.broadcast_event(device_id, user_left_event, client_id).await {
                    error!("Failed to broadcast user left event: {}", e);
                }
            } else {
                // Multi-Tab Fix: Send refresh signal to update connection counts when user reduces tabs
                let refresh_event = crate::events::DeviceEvent::user_left("USER_COUNT_REFRESH".to_string(), "".to_string(), "".to_string());
                if let Err(e) = self.broadcast_event(device_id, refresh_event, client_id).await {
                    error!("Failed to broadcast connection count refresh event: {}", e);
                }
            }
        }
        
        // ESP32 devices don't have shape selections to clean up
        debug!("Client {} disconnected from device {}", client_id, device_id);
        
        Ok(())
    }
    
    /// Get count of active connections for a device
    pub async fn get_connection_count(&self, device_id: &str) -> usize {
        let connections = self.active_connections.read().await;
        connections.get(device_id).map(|v| v.len()).unwrap_or(0)
    }
    
    /// Get all active devices with their connection counts
    pub async fn get_active_devices(&self) -> HashMap<String, usize> {
        let connections = self.active_connections.read().await;
        connections.iter()
            .map(|(device_id, connections)| (device_id.clone(), connections.len()))
            .collect()
    }
    
    /// Get all users currently connected to a device
    pub async fn get_device_users(&self, device_id: &str) -> Vec<DeviceUser> {
        let connections = self.active_connections.read().await;
        
        if let Some(device_connections) = connections.get(device_id) {
            // Group connections by user_id to count multiple connections and get color
            let mut user_map: std::collections::HashMap<String, (String, String, usize)> = std::collections::HashMap::new();
            
            for connection in device_connections {
                let entry = user_map.entry(connection.user_id.clone())
                    .or_insert((connection.display_name.clone(), connection.user_color.clone(), 0));
                entry.2 += 1;
            }
            
            user_map.into_iter()
                .map(|(user_id, (display_name, user_color, connection_count))| DeviceUser {
                    user_id,
                    display_name,
                    connection_count,
                    user_color,
                })
                .collect()
        } else {
            Vec::new()
        }
    }
    
    /// Get all users currently connected to a device with database lookup for display names
    pub async fn get_device_users_with_db(&self, device_id: &str, db: &crate::database::DatabaseManager) -> Vec<DeviceUser> {
        let connections = self.active_connections.read().await;
        
        if let Some(device_connections) = connections.get(device_id) {
            // Group connections by user_id to count multiple connections and get color
            let mut user_map: std::collections::HashMap<String, (String, usize)> = std::collections::HashMap::new();
            
            for connection in device_connections {
                let entry = user_map.entry(connection.user_id.clone())
                    .or_insert((connection.user_color.clone(), 0));
                entry.1 += 1;
            }
            
            // Collect results with database lookup for display names
            let mut users = Vec::new();
            for (user_id, (user_color, connection_count)) in user_map {
                // Try to get display name from database
                let display_name = match db.get_user_by_id(&user_id).await {
                    Ok(Some(user)) => user.display_name,
                    _ => user_id.clone() // Fallback to user_id
                };
                
                users.push(DeviceUser {
                    user_id,
                    display_name,
                    connection_count,
                    user_color,
                });
            }
            
            users
        } else {
            Vec::new()
        }
    }
    
    // ========================================================================
    // EVENT BROADCASTING
    // ========================================================================
    
    /// Broadcast an event to all connected clients on a device (except sender)
    /// Multi-tab support: Sends to all clients including other tabs of same user
    /// Subscription filtering: Light subscriptions only receive connection status events
    pub async fn broadcast_event(
        &self,
        device_id: &str,
        event: DeviceEvent,
        sender_client_id: &str
    ) -> Result<(), String> {
        let connections = self.active_connections.read().await;

        if let Some(device_connections) = connections.get(device_id) {
            // Check if this event should be sent to light subscriptions
            let is_connection_status = matches!(event, DeviceEvent::Esp32ConnectionStatus { .. });

            let message = ServerMessage::device_events(
                device_id.to_string(),
                vec![event]
            );

            let mut successful_sends = 0;
            let mut failed_sends = 0;

            for connection in device_connections {
                // Don't send event back to the exact sender client
                // But do send to other tabs of the same user (different client_id)
                if connection.client_id == sender_client_id {
                    continue;
                }

                // Filter events based on subscription type
                if connection.subscription_type == crate::events::SubscriptionType::Light && !is_connection_status {
                    debug!("SUBSCRIPTION FILTER: Skipping non-connection event for Light subscription client {} on device {}",
                           connection.client_id, device_id);
                    continue;
                }

                match connection.send_message(message.clone()) {
                    Ok(()) => successful_sends += 1,
                    Err(e) => {
                        error!("Failed to broadcast to client {}: {}", connection.client_id, e);
                        failed_sends += 1;
                    }
                }
            }

            if successful_sends == 0 && failed_sends == 0 {
                warn!("NO clients received the event for device {} - frontend may show 'Disconnected'!", device_id);
            }
            
            // TODO: Clean up failed connections in a background task
        }
        
        Ok(())
    }
    
    
    // ========================================================================
    // CLEANUP & MAINTENANCE
    // ========================================================================
    
    /// Remove stale connections (connections where the sender channel is closed)
    pub async fn cleanup_stale_connections(&self) -> usize {
        let mut connections = self.active_connections.write().await;
        let mut removed_count = 0;
        
        // Check each device
        let device_ids: Vec<String> = connections.keys().cloned().collect();
        
        for device_id in device_ids {
            if let Some(device_connections) = connections.get_mut(&device_id) {
                let initial_count = device_connections.len();
                
                // Keep only connections with open channels
                device_connections.retain(|conn| !conn.sender.is_closed());
                
                let removed_for_device = initial_count - device_connections.len();
                removed_count += removed_for_device;
                
                if removed_for_device > 0 {
                    debug!("Removed {} stale connections from device {}", removed_for_device, device_id);
                }
                
                // Remove empty device entries
                if device_connections.is_empty() {
                    connections.remove(&device_id);
                }
            }
        }
        
        if removed_count > 0 {
            info!("Cleaned up {} stale connections", removed_count);
        }
        
        removed_count
    }
    
    /// Get storage statistics for monitoring
    pub async fn get_stats(&self) -> DeviceStoreStats {
        let events = self.device_events.read().await;
        let connections = self.active_connections.read().await;
        
        let total_events: usize = events.values().map(|v| v.len()).sum();
        let total_connections: usize = connections.values().map(|v| v.len()).sum();
        
        DeviceStoreStats {
            total_devices: events.len(),
            total_events,
            active_devices: connections.len(),
            total_connections,
            average_events_per_device: if events.is_empty() { 0.0 } else { total_events as f64 / events.len() as f64 },
            average_connections_per_device: if connections.is_empty() { 0.0 } else { total_connections as f64 / connections.len() as f64 },
        }
    }
}

// ============================================================================
// STATISTICS & MONITORING
// ============================================================================

#[derive(Debug, Clone)]
pub struct DeviceStoreStats {
    pub total_devices: usize,
    pub total_events: usize,
    pub active_devices: usize,
    pub total_connections: usize,
    pub average_events_per_device: f64,
    pub average_connections_per_device: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceUser {
    pub user_id: String,
    pub display_name: String,
    pub connection_count: usize,
    pub user_color: String,
}

impl Default for DeviceEventStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CONVENIENCE TYPE ALIASES
// ============================================================================

pub type SharedDeviceStore = Arc<DeviceEventStore>;

/// Create a new shared device store instance
pub fn create_shared_store() -> SharedDeviceStore {
    Arc::new(DeviceEventStore::new())
}

