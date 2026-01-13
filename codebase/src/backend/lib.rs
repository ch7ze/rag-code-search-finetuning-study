// ============================================================================
// LIB.RS - LIBRARY EXPORTS FOR TESTING
// Makes internal modules available as library for integration tests
// ============================================================================

use std::sync::Arc;
use axum::{Router, Json, extract::State, http::StatusCode};
use serde_json::{json, Value};

// All modules
pub mod auth;
pub mod file_utils;
pub mod database;
pub mod device_store;
pub mod events;
pub mod websocket;
pub mod esp32_types;
pub mod esp32_connection;
pub mod esp32_manager;
pub mod esp32_discovery;
pub mod mdns_discovery;
pub mod mdns_server;
pub mod debug_logger;
pub mod uart_connection;

// Re-export key types for tests
pub use database::DatabaseManager;
pub use device_store::{create_shared_store, SharedDeviceStore};

// Create a test-friendly app instance
pub async fn create_test_app() -> Router {
    // Initialize minimal components for testing
    let db = Arc::new(DatabaseManager::new().await.expect("Failed to create test database"));
    let device_store = create_shared_store();
    let esp32_manager = esp32_manager::create_esp32_manager(device_store.clone());

    // Start ESP32 manager for tests
    esp32_manager.start().await;

    let esp32_discovery = Arc::new(tokio::sync::Mutex::new(
        esp32_discovery::Esp32Discovery::with_manager(device_store.clone(), Some(esp32_manager.clone()))
    ));

    let mdns_server = Arc::new(tokio::sync::Mutex::new(
        mdns_server::MdnsServer::new().expect("Failed to create test mDNS server")
    ));

    // Add test ESP32 device for consistent testing
    let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 43, 75));
    let test_device = esp32_types::Esp32DeviceConfig::new(
        "test-esp32-001".to_string(),
        ip,
        3232,
        3232,
    );
    let _ = esp32_manager.add_device(test_device).await;

    // Create app using the function from main.rs
    create_app_internal(db, device_store, esp32_manager, esp32_discovery, mdns_server).await
}

// Define AppState for dependency injection
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<DatabaseManager>,
    pub device_store: SharedDeviceStore,
    pub esp32_manager: Arc<esp32_manager::Esp32Manager>,
    pub esp32_discovery: Arc<tokio::sync::Mutex<esp32_discovery::Esp32Discovery>>,
    pub mdns_server: Arc<tokio::sync::Mutex<mdns_server::MdnsServer>>,
}

// Copy the create_app function logic here for testing
// This avoids circular imports with main.rs
async fn create_app_internal(
    db: Arc<DatabaseManager>,
    device_store: SharedDeviceStore,
    esp32_manager: Arc<esp32_manager::Esp32Manager>,
    esp32_discovery: Arc<tokio::sync::Mutex<esp32_discovery::Esp32Discovery>>,
    mdns_server: Arc<tokio::sync::Mutex<mdns_server::MdnsServer>>
) -> Router {
    use axum::routing::get;
    use tower::ServiceBuilder;
    use tower_http::trace::TraceLayer;

    let mut app = Router::new();

    // AppState for all handlers
    let app_state = AppState {
        db: db.clone(),
        device_store: device_store.clone(),
        esp32_manager: esp32_manager.clone(),
        esp32_discovery: esp32_discovery.clone(),
        mdns_server: mdns_server.clone(),
    };

    // API Routes
    let api_routes = Router::new()
        .route("/api", get(api_home))
        .route("/api/users", get(api_users))
        .route("/api/esp32/discovered", get(discovered_esp32_devices_handler))
        .route("/api/devices", get(list_devices_handler))
        .with_state(app_state.clone());

    // WebSocket routes
    let websocket_routes = Router::new()
        .route("/channel", get(websocket_handler))
        .with_state(app_state.clone());

    // Merge routes
    app = app.merge(api_routes);
    app = app.merge(websocket_routes);

    // Add middleware
    app = app.layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
    );

    app
}

// Handler functions
async fn api_home() -> Json<Value> {
    Json(json!({
        "title": "ESP32 Manager Backend",
        "status": "running",
        "version": "0.1.0"
    }))
}

async fn api_users() -> Json<Value> {
    Json(json!({ "users": [] }))
}

async fn discovered_esp32_devices_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    // Get discovered devices from ESP32Discovery service
    let discovered_devices = {
        let discovery = app_state.esp32_discovery.lock().await;
        discovery.get_discovered_devices().await
    };

    // Convert to JSON format expected by tests
    let devices: Vec<Value> = discovered_devices
        .into_iter()
        .map(|(name, device)| {
            json!({
                "name": name,
                "ip": device.device_config.ip_address.to_string(),
                "tcp_port": device.device_config.tcp_port,
                "udp_port": device.udp_port
            })
        })
        .collect();

    Ok(Json(json!({
        "devices": devices,
        "count": devices.len()
    })))
}

async fn list_devices_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    // Get devices from device store
    let devices = app_state.device_store.get_active_devices().await;

    Ok(Json(json!({
        "devices": devices,
        "count": devices.len()
    })))
}

async fn websocket_handler() -> &'static str {
    "WebSocket endpoint - use proper WebSocket client to connect"
}