// ============================================================================
// IMPORTS - These lines import external libraries and modules
// ============================================================================

// Axum is the web framework for Rust - similar to Express.js for Node.js
use axum::{
    body::Body,                     // HTTP Body for responses
    extract::{Path, State},         // Path for URL parameters, State for global state
    http::StatusCode,        // HTTP Status Codes (200, 404, etc.)
    response::{IntoResponse, Response}, // Traits for HTTP responses
    routing::{get, post, Router},   // HTTP Routing (GET /login, POST /api/register)
    Json,                           // JSON Parser for API requests/responses
};
// Axum Extra for extended features
use axum_extra::extract::CookieJar; // For reading browser cookies

// Serde for JSON serialization/deserialization
use serde::Deserialize;
use serde_json::{json, Value};      // JSON handling

// Standard Rust libraries
use std::{fs, sync::Arc}; // File system, Arc for thread-safe references
use pulldown_cmark::{Parser, html}; // Markdown parsing

// Tower for middleware (logging, etc.)
use tower::ServiceBuilder;
use tower_http::{
    services::ServeDir,             // Serve static files (CSS, JS, HTML)
    trace::TraceLayer,              // HTTP Request Logging
};

// ============================================================================
// MODULE IMPORTS - Unsere eigenen Code-Module
// ============================================================================

mod auth;        // auth.rs - Authentication (Login, Register, JWT)
mod file_utils;  // file_utils.rs - File handling and SPA routing
mod database;    // database.rs - SQLite database integration
mod events;      // events.rs - Event definitions for ESP32 Devices
mod device_store; // device_store.rs - In-Memory Event Store for ESP32 devices
mod websocket;   // websocket.rs - WebSocket handler for multiuser
mod esp32_types; // esp32_types.rs - ESP32 communication types
mod esp32_connection; // esp32_connection.rs - ESP32 TCP/UDP connection handling
mod esp32_manager; // esp32_manager.rs - ESP32 device management
mod mdns_discovery; // mdns_discovery.rs - mDNS-based ESP32 discovery
mod mdns_server;    // mdns_server.rs - mDNS server for advertising esp-server.local
mod esp32_discovery; // esp32_discovery.rs - ESP32 device discovery service
mod debug_logger;   // debug_logger.rs - Debug event logging
mod uart_connection; // uart_connection.rs - UART/Serial connection handling

// Import all authentication functions from auth.rs
// These are used for Login/Register/Logout on the website
use auth::{
    create_auth_cookie,    // Creates secure HTTP cookies for logged-in users
    create_jwt,           // Creates JSON Web Tokens for authentication  
    create_logout_cookie, // Deletes auth cookies on logout
    validate_jwt,         // Checks if JWT token is still valid
    AuthResponse,         // Struct for API responses (success: true/false, message)
    LoginRequest,         // Struct for login data from frontend (email, password)
    RegisterRequest,      // Struct for registration data
    UpdateDisplayNameRequest, // Struct for display name updates
    User,                // User data structure with hashed passwords
    // A 5.4: ESP32-Device-Management Imports
    CreateDeviceRequest, // Request for new ESP32 device
    UpdateDeviceRequest, // Request for device updates
    UpdatePermissionRequest, // Request for permission updates
};

// Import all file handling functions
// These are used for serving website files
use file_utils::handle_template_file;

// Import database functions
use database::{DatabaseManager};

// Import Event Store and WebSocket functions
use device_store::{create_shared_store, SharedDeviceStore};
use websocket::{websocket_handler, websocket_stats_handler, device_users_handler, start_cleanup_task, WebSocketState};

// DEBUG: Simple test handler for WebSocket routing
async fn debug_websocket_handler() -> Result<String, (axum::http::StatusCode, String)> {
    tracing::error!("DEBUG WebSocket handler called!");
    Ok("DEBUG: WebSocket handler reached".to_string())
}

// ============================================================================
// APP STATE - Global state for the application
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<DatabaseManager>,
    pub device_store: SharedDeviceStore,
    pub esp32_manager: Arc<esp32_manager::Esp32Manager>,
    pub esp32_discovery: Arc<tokio::sync::Mutex<esp32_discovery::Esp32Discovery>>,
    pub mdns_server: Arc<tokio::sync::Mutex<mdns_server::MdnsServer>>,
    pub uart_connection: Arc<tokio::sync::Mutex<uart_connection::UartConnection>>,
}

// ============================================================================
// MAIN FUNCTION - Entry point of our Rust web application
// Website feature: Starts the complete web server
// ============================================================================

#[tokio::main]  // This attribute makes main() async-capable with Tokio runtime
async fn main() {
    // Enhanced logging configuration with environment variable support
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(true)
        .with_line_number(true)
        .with_file(true)
        .init();

    tracing::info!("Starting Drawing App Backend Server");

    // Clear debug log file for fresh start
    debug_logger::DebugLogger::clear_log();


    // Initialize SQLite database
    tracing::info!("Initializing SQLite database...");
    let db_exists = std::path::Path::new("data/users.db").exists();
    
    let db = match DatabaseManager::new().await {
        Ok(db) => {
            if db_exists {
                tracing::info!("Connected to existing SQLite database: data/users.db");
            } else {
                tracing::info!("Created new SQLite database: data/users.db");
            }
            Arc::new(db)
        }
        Err(e) => {
            tracing::error!("Failed to initialize database: {:?}", e);
            panic!("Database initialization failed");
        }
    };

    // Initialize Device Event Store
    tracing::info!("Initializing Device Event Store...");
    let device_store = create_shared_store();

    // Load debug settings and configure device store
    if let Ok(Some(max_debug_messages)) = db.get_debug_settings().await {
        device_store.set_max_debug_messages(max_debug_messages as usize).await;
        tracing::info!("Loaded debug settings: max_debug_messages={}", max_debug_messages);
    } else {
        tracing::info!("Using default debug settings: max_debug_messages=200");
    }
    
    // Initialize ESP32 Manager
    tracing::info!("Initializing ESP32 Manager...");


    let esp32_manager = esp32_manager::create_esp32_manager(device_store.clone());
    esp32_manager.start().await;
    
    // Start ESP32 Discovery Service
    tracing::info!("Starting ESP32 Discovery Service...");
    let esp32_discovery = Arc::new(tokio::sync::Mutex::new(esp32_discovery::Esp32Discovery::with_manager(device_store.clone(), Some(esp32_manager.clone()))));
    let discovery_service = esp32_discovery.clone();
    tokio::spawn(async move {
        let mut discovery = discovery_service.lock().await;
        if let Err(e) = discovery.start_discovery().await {
            tracing::error!("ESP32 discovery failed to start: {}", e);
        } else {
            tracing::info!("ESP32 discovery service started successfully");
        }
    });

    // Start mDNS Server for advertising esp-server.local
    tracing::info!("Starting mDNS Server...");
    let mdns_server = Arc::new(tokio::sync::Mutex::new(
        mdns_server::MdnsServer::new().map_err(|e| {
            tracing::error!("Failed to create mDNS server: {}", e);
            e
        }).unwrap()
    ));

    let mdns_service = mdns_server.clone();
    tokio::spawn(async move {
        let mut server = mdns_service.lock().await;
        if let Err(e) = server.start_advertising(3000).await {
            tracing::error!("mDNS server failed to start: {}", e);
        } else {
            tracing::info!("mDNS server started - esp-server.local advertised on port 3000");
        }
    });
    
    // Example: Add a test ESP32 device configuration for testing
    let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 43, 75));
    let test_device = esp32_types::Esp32DeviceConfig::new(
        "test-esp32-001".to_string(),
        ip,
        3232, // ESP32 TCP port
        3232, // ESP32 UDP port
    );
    if let Err(e) = esp32_manager.add_device(test_device).await {
        tracing::warn!("Failed to add test ESP32 device: {}", e);
    } else {
        tracing::info!("Added test ESP32 device: test-esp32-001 (192.168.43.75)");
    }

    // Add test device with colons to see if that causes the Event-Forwarding-Task termination issue
    let ip_colon_test = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 43, 76));
    let test_device_with_colons = esp32_types::Esp32DeviceConfig::new(
        "test:colon:device".to_string(),
        ip_colon_test,
        3232, // ESP32 TCP port
        3232, // ESP32 UDP port
    );
    if let Err(e) = esp32_manager.add_device(test_device_with_colons).await {
        tracing::warn!("Failed to add test device with colons: {}", e);
    } else {
        tracing::info!("Added test device with colons: test:colon:device (192.168.43.76)");
    }
    
    // Start WebSocket cleanup task
    let cleanup_store = device_store.clone();
    tokio::spawn(async move {
        start_cleanup_task(cleanup_store).await;
    });
    tracing::info!("Started WebSocket cleanup task");

    // Initialize UART Connection with shared state trackers from ESP32Manager
    tracing::info!("Initializing UART connection...");
    let uart_connection = Arc::new(tokio::sync::Mutex::new(
        uart_connection::UartConnection::new(
            device_store.clone(),
            esp32_manager.get_unified_connection_states(),
            esp32_manager.get_unified_activity_tracker(),
            esp32_manager.get_device_connection_types(),
        )
    ));

    // Try to auto-connect UART if settings exist
    if let Ok(Some((port, baud_rate, auto_connect))) = db.get_uart_settings().await {
        if auto_connect && port.is_some() {
            let port_name = port.unwrap();
            tracing::info!("Auto-connecting to UART port {} at {} baud", port_name, baud_rate);
            let mut uart = uart_connection.lock().await;
            match uart.connect(port_name.clone(), baud_rate).await {
                Ok(()) => {
                    tracing::info!("UART auto-connect successful: {}", port_name);
                }
                Err(e) => {
                    tracing::warn!("UART auto-connect failed for port {}: {}", port_name, e);
                }
            }
        }
    }

    // Create web app with all routes
    tracing::info!("Creating application routes...");
    let app = create_app(db, device_store, esp32_manager, esp32_discovery, mdns_server, uart_connection).await;

    // Start TCP listener on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();  // unwrap() = stop program on error
    
    tracing::info!("Server running on http://0.0.0.0:3000 (accessible via localhost:3000 or 127.0.0.1:3000)");
    tracing::info!("Available endpoints:");
    tracing::info!("   - GET  /           - SPA Main Page");
    tracing::info!("   - GET  /login.html - Login Page");
    tracing::info!("   - POST /api/login  - Login API");
    tracing::info!("   - POST /api/register - Register API");
    tracing::info!("   - POST /api/profile/display-name - Update Display Name");
    tracing::info!("   - GET  /channel    - WebSocket Canvas Events");
    tracing::info!("   - GET  /api/websocket/stats - WebSocket Statistics");
    tracing::info!("Debug tip: Set RUST_LOG=debug for detailed logging");
    
    // Start server and wait for requests - with ConnectInfo for WebSocket
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
}

// ============================================================================
// APP CREATION - Creates the web router with all routes
// Website feature: Defines all URLs and their handler functions
// ============================================================================

pub async fn create_app(db: Arc<DatabaseManager>, device_store: SharedDeviceStore, esp32_manager: Arc<esp32_manager::Esp32Manager>, esp32_discovery: Arc<tokio::sync::Mutex<esp32_discovery::Esp32Discovery>>, mdns_server: Arc<tokio::sync::Mutex<mdns_server::MdnsServer>>, uart_connection: Arc<tokio::sync::Mutex<uart_connection::UartConnection>>) -> Router {
    let mut app = Router::new();

    // AppState for all handlers
    let app_state = AppState {
        db: db.clone(),
        device_store: device_store.clone(),
        esp32_manager: esp32_manager.clone(),
        esp32_discovery: esp32_discovery.clone(),
        mdns_server: mdns_server.clone(),
        uart_connection: uart_connection.clone(),
    };

    // WebSocket State for WebSocket handlers
    let websocket_state = WebSocketState {
        device_store: device_store.clone(),
        db: db.clone(),
        esp32_manager: esp32_manager.clone(),
        esp32_discovery: esp32_discovery.clone(),
        uart_connection: uart_connection.clone(),
    };

    // ========================================
    // API ROUTES - Backend APIs for frontend
    // ========================================
    // These routes are called by JavaScript in the frontend
    let api_routes = Router::new()
        // GET /api - Basic info about the API
        .route("/api", get(api_home))
        
        // GET /api/users - List all users (currently empty)
        .route("/api/users", get(api_users))
        
        // POST /api/register - Register new user
        // Called by register.html
        .route("/api/register", post(register_handler))
        
        // POST /api/login - Log in user
        // Called by login.html
        .route("/api/login", post(login_handler))
        
        // POST /api/logout - Log out user
        // Called by logout button
        .route("/api/logout", post(logout_handler))
        
        // GET /api/validate-token - Check if user is logged in
        // Called by app.js for authentication check
        .route("/api/validate-token", get(validate_token_handler))
        
        // GET /api/user-info - Returns user information from JWT
        // Used for display name display
        .route("/api/user-info", get(user_info_handler))
        
        // PUT /api/profile/display-name - Change display name
        // Used for profile updates
        .route("/api/profile/display-name", post(update_display_name_handler))
        
        // ========================================
        // A 5.4: ESP32 DEVICE MANAGEMENT API ROUTES
        // ========================================
        
        // GET /api/devices - List all devices of logged-in user
        .route("/api/devices", get(list_devices_handler))
        
        // POST /api/devices - Create new ESP32 device
        .route("/api/devices", post(create_device_handler))
        
        // GET /api/devices/:id - Details of an ESP32 device
        .route("/api/devices/:id", get(get_device_handler).put(update_device_handler).delete(delete_device_handler))
        
        // POST /api/device-permissions/:id - Manage permissions for a device
        .route("/api/device-permissions/:id", post(simple_permissions_handler))
        
        // GET /api/esp32/discovered - List discovered ESP32 devices  
        .route("/api/esp32/discovered", get(discovered_esp32_devices_handler))
        
        // GET /api/users/search - Search for users for permission management
        .route("/api/users/search", get(search_users_handler))
        
        // GET /api/users/list - Get first users for scroll field
        .route("/api/users/list", get(list_users_handler))
        
        // GET /api/docs - Get documentation content for SPA
        .route("/api/docs", get(api_docs_handler))
        // GET /api/docs/:path - Get specific documentation files
        .route("/api/docs/*path", get(api_docs_file_handler))

        // ========================================
        // UART SETTINGS API ROUTES
        // ========================================

        // GET /api/uart/settings - Get current UART settings
        .route("/api/uart/settings", get(get_uart_settings_handler))

        // POST /api/uart/settings - Update UART settings
        .route("/api/uart/settings", post(update_uart_settings_handler))

        // GET /api/uart/ports - List available serial ports
        .route("/api/uart/ports", get(list_uart_ports_handler))

        // POST /api/uart/connect - Connect to UART port
        .route("/api/uart/connect", post(uart_connect_handler))

        // POST /api/uart/disconnect - Disconnect from UART port
        .route("/api/uart/disconnect", post(uart_disconnect_handler))

        // GET /api/uart/status - Get UART connection status
        .route("/api/uart/status", get(uart_status_handler))

        // ========================================
        // DEBUG SETTINGS API ROUTES
        // ========================================

        // GET /api/debug/settings - Get current debug settings
        .route("/api/debug/settings", get(get_debug_settings_handler).post(update_debug_settings_handler))

        // with_state() gives all API routes access to both stores
        .with_state(app_state);

    // ========================================
    // WEBSOCKET ROUTES - A 5.5 Multiuser Support
    // ========================================
    let websocket_routes = Router::new()
        // WebSocket endpoint for Canvas Events - A 5.5 requirement: ws://.../channel/
        .route("/channel", get(websocket_handler))
        
        // Debug endpoint to test routing
        .route("/channel/debug", get(debug_websocket_handler))
        
        // WebSocket statistics endpoint for monitoring/debugging
        .route("/api/websocket/stats", get(websocket_stats_handler))
        
        // Get users connected to a device
        .route("/api/devices/:device_id/users", get(device_users_handler))
        
        .with_state(websocket_state);

    // Add API routes to main router
    app = app.merge(api_routes);
    
    // Add WebSocket routes to main router
    app = app.merge(websocket_routes);

    // Handle outdated hash URLs - redirect middleware would go here
    // For now, we'll handle this in the catch-all

    // Serve static files from 'public' directory (no hash versioning)
    app = app.nest_service("/stylesheets", ServeDir::new("public/stylesheets"));

    // SPA routes - all serve the same index.html shell
    app = app
        .route("/index.html", get(serve_spa_route))
        .route("/login.html", get(serve_spa_route))
        .route("/register.html", get(serve_spa_route))
        .route("/debug.html", get(serve_spa_route))
        .route("/hallo.html", get(serve_spa_route))
        .route("/about.html", get(serve_spa_route))
        .route("/drawing_board.html", get(serve_spa_route))
        .route("/drawer_page.html", get(serve_spa_route))
        .route("/esp32_control.html", get(serve_spa_route))
        .route("/docs.html", get(serve_spa_route))
        .route("/settings.html", get(serve_spa_route));


    // Serve static files directly from 'client' directory with development-friendly caching
    app = app.nest_service("/templates", ServeDir::new("client/templates"));
    app = app.route("/scripts/*path", get(serve_script_file));
    app = app.route("/styles/*path", get(serve_style_file));
    
    // Note: /docs is now handled as SPA route, markdown API available at /api/docs


    // Root path serves SPA
    app = app.route("/", get(serve_spa_route));

    // Serve remaining static files from client with development-friendly caching
    app = app
        .route("/index.css", get(|| serve_dev_static_file("client/index.css", "text/css", "max-age=0, must-revalidate")))
        .route("/app.js", get(|| serve_dev_static_file("client/app.js", "text/javascript", "max-age=0, must-revalidate")));

    // SPA routes for specific paths
    app = app
        .route("/login", get(serve_spa_route))
        .route("/register", get(serve_spa_route))
        .route("/hallo", get(serve_spa_route))
        .route("/about", get(serve_spa_route))
        .route("/drawing_board", get(serve_spa_route))
        .route("/drawer_page", get(serve_spa_route))
        .route("/debug", get(serve_spa_route))
        .route("/index", get(serve_spa_route))
        .route("/docs", get(serve_spa_route))
        .route("/settings", get(serve_spa_route));

    // Device routes - more specific to avoid catching static files
    app = app
        .route("/devices", get(serve_spa_route))
        .route("/devices/:device_id", get(serve_spa_route));

    // Add middleware
    app = app.layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
    );

    app
}

// ============================================================================
// API HANDLER FUNCTIONS - These functions process HTTP requests
// ============================================================================

// GET /api - Basic API info
// Website feature: API documentation/status
async fn api_home() -> Json<Value> {
    // json!() is a macro that creates JSON
    Json(json!({ "title": "Express" }))
}

// GET /api/users - List all users  
// Website feature: User management (currently not used)
async fn api_users() -> Json<Value> {
    Json(json!({ "users": [] }))
}

// SPA route handler - always serves the main SPA shell (index.html)
async fn serve_spa_route() -> Response<Body> {
    // HTML sollte nicht gecacht werden, damit SPA-Updates funktionieren
    handle_template_file("client/index.html", "no-cache, must-revalidate").await
}

// Handler für JavaScript-Dateien mit entwicklungsfreundlichem Caching
async fn serve_script_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response<Body> {
    let file_path = format!("client/scripts/{}", path);
    serve_dev_static_file(&file_path, "text/javascript", "max-age=0, must-revalidate").await
}

// Handler für CSS-Dateien mit entwicklungsfreundlichem Caching
async fn serve_style_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response<Body> {
    let file_path = format!("client/styles/{}", path);
    serve_dev_static_file(&file_path, "text/css", "max-age=0, must-revalidate").await
}

// Allgemeine Funktion für statische Dateien mit entwicklungsfreundlichem Caching
async fn serve_dev_static_file(file_path: &str, content_type: &str, cache_control: &str) -> Response<Body> {
    match std::fs::read_to_string(file_path) {
        Ok(contents) => {
            Response::builder()
                .header("content-type", content_type)
                .header("cache-control", cache_control)
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap()
        }
    }
}




// GET /api/docs - Get main documentation content for SPA
async fn api_docs_handler() -> impl IntoResponse {
    serve_markdown_file("docs/README.md").await
}

// GET /api/docs/:path - Get specific documentation files
async fn api_docs_file_handler(Path(path): Path<String>) -> impl IntoResponse {
    let file_path = if path.is_empty() || path == "/" || path == "" {
        "docs/README.md".to_string()
    } else if path.ends_with('/') {
        format!("docs/{}README.md", path)
    } else if path.ends_with(".md") {
        format!("docs/{}", path)
    } else {
        format!("docs/{}.md", path)
    };
    
    tracing::debug!("API docs request: path='{}' -> file_path='{}'", path, file_path);
    
    serve_markdown_file(&file_path).await
}

// Common markdown file serving function
async fn serve_markdown_file(file_path: &str) -> impl IntoResponse {
    match fs::read_to_string(file_path) {
        Ok(markdown_content) => {
            let parser = Parser::new(&markdown_content);
            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);

            let full_html = format!(
                r#"<!DOCTYPE html>
<html lang="de">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Dokumentation</title>
    <style>
        body {{ 
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 900px;
            margin: 0 auto;
            padding: 20px;
            background: #fff;
        }}
        pre {{ 
            background: #f4f4f4;
            padding: 15px;
            border-radius: 5px;
            overflow-x: auto;
        }}
        code {{ 
            background: #f4f4f4;
            padding: 2px 5px;
            border-radius: 3px;
            font-family: 'Monaco', 'Consolas', monospace;
        }}
        h1, h2, h3 {{ 
            border-bottom: 1px solid #eee;
            padding-bottom: 10px;
        }}
        blockquote {{
            border-left: 4px solid #ddd;
            margin: 0;
            padding-left: 20px;
            color: #666;
        }}
        table {{
            border-collapse: collapse;
            width: 100%;
            margin: 20px 0;
        }}
        th, td {{
            border: 1px solid #ddd;
            padding: 8px;
            text-align: left;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        a {{
            color: #0066cc;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div style="margin-bottom: 20px;">
        <a href="/docs/">← Zurück zur Dokumentation</a> | 
        <a href="/" target="_blank">← Zurück zur App</a>
    </div>
    {}
</body>
</html>"#,
                html_output
            );

            Response::builder()
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(full_html))
                .unwrap()
        }
        Err(_) => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Dokumentation nicht gefunden"))
                .unwrap()
        }
    }
}




// ============================================================================
// AUTHENTICATION HANDLERS - Functions for Login/Register/Logout
// Website feature: User registration and login
// ============================================================================

// POST /api/register - Register new user
// Called when someone submits the registration form
async fn register_handler(
    // State(app_state) extracts the global app state from the request
    State(app_state): State<AppState>,
    // Json(req) parses the JSON request body into RegisterRequest struct
    Json(req): Json<RegisterRequest>,
) -> Result<Response<Body>, StatusCode> {  // Return: HTTP Response or error
    
    tracing::info!("Registration attempt for email: {}", req.email);
    tracing::debug!("Register request received: {:?}", req.email);
    
    // Step 1: Check if user already exists
    match app_state.db.get_user_by_email(&req.email).await {
        Ok(Some(_)) => {
            tracing::warn!("Registration failed: User {} already exists", req.email);
            let response = AuthResponse {
                success: false,
                message: "User already exists".to_string(),
                email: None,
            };
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)  // HTTP 400
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        }
        Ok(None) => {
            // User does not exist - continue with registration
        }
        Err(e) => {
            tracing::error!("Database error during user lookup: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Step 2: Create new DatabaseUser
    tracing::debug!("Creating new user with hashed password");
    let db_user = match database::DatabaseUser::new(req.email.clone(), req.display_name.clone(), &req.password) {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("User creation failed for {}: {:?}", req.email, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Step 3: Save user to database
    if let Err(e) = app_state.db.create_user(db_user.clone()).await {
        tracing::error!("Database error during user creation: {:?}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Step 4: Convert user for JWT
    let user = User {
        id: db_user.id.clone(),
        email: db_user.email.clone(),
        display_name: db_user.display_name.clone(),
        password_hash: db_user.password_hash.clone(),
    };

    // Step 5: Create JWT token (auto-login after registration)
    tracing::debug!("Creating JWT token for new user");
    match create_jwt(&user) {
        Ok(token) => {
            tracing::info!("Registration successful for user: {}", req.email);
            let response = AuthResponse {
                success: true,
                message: "User registered successfully".to_string(),
                email: Some(req.email.clone()),
            };

            Response::builder()
                .header("set-cookie", create_auth_cookie(&token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(e) => {
            tracing::error!("JWT creation failed for {}: {:?}", req.email, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn login_handler(
    State(app_state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Response<Body>, StatusCode> {
    
    tracing::info!("Login attempt for email: {}", req.email);
    tracing::debug!("Login request received for: {}", req.email);
    
    // Search for user in database
    let db_user = match app_state.db.get_user_by_email(&req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            tracing::warn!("Login failed: User {} not found", req.email);
            let response = AuthResponse {
                success: false,
                message: "Invalid credentials".to_string(),
                email: None,
            };
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(e) => {
            tracing::error!("Database error during login for {}: {:?}", req.email, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    tracing::debug!("User found in database: {}", req.email);
    
    // Verify password
    match db_user.verify_password(&req.password) {
        Ok(true) => {
            tracing::debug!("Password verification successful");
            
            // Convert user for JWT
            let user = User {
                id: db_user.id.clone(),
                email: db_user.email.clone(),
                display_name: db_user.display_name.clone(),
                password_hash: db_user.password_hash.clone(),
            };
            
            // Create JWT token
            match create_jwt(&user) {
                Ok(token) => {
                    tracing::info!("Login successful for user: {}", req.email);
                    let response = AuthResponse {
                        success: true,
                        message: "Login successful".to_string(),
                        email: Some(req.email.clone()),
                    };

                    Response::builder()
                        .header("set-cookie", create_auth_cookie(&token))
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_string(&response).unwrap()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
                }
                Err(e) => {
                    tracing::error!("JWT creation failed during login for {}: {:?}", req.email, e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Ok(false) => {
            tracing::warn!("Login failed: Invalid password for {}", req.email);
            let response = AuthResponse {
                success: false,
                message: "Invalid credentials".to_string(),
                email: None,
            };
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(e) => {
            tracing::error!("Password verification error for {}: {:?}", req.email, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn logout_handler() -> Response<Body> {
    let response = AuthResponse {
        success: true,
        message: "Logged out successfully".to_string(),
        email: None,
    };

    Response::builder()
        .header("set-cookie", create_logout_cookie())
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&response).unwrap()))
        .unwrap()
}

async fn validate_token_handler(cookie_jar: CookieJar) -> StatusCode {
    // Always return OK since authentication is now optional
    // The frontend can continue to use this endpoint to check authentication
    // but it will always succeed allowing access without login
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    match token {
        Some(token_value) => {
            // If there is a token, validate it
            match validate_jwt(token_value) {
                Ok(_) => StatusCode::OK,
                Err(_) => StatusCode::OK, // Even invalid tokens are OK now (guest access)
            }
        }
        None => StatusCode::OK, // No token is also OK (guest access)
    }
}

// GET /api/user-info - Returns user information from JWT (optional auth)
// Website feature: Display name display in frontend
async fn user_info_handler(cookie_jar: CookieJar) -> Result<Json<Value>, StatusCode> {
    // Extract JWT token from cookie (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());

    match token {
        Some(token_value) => {
            // If token exists, validate and return user info
            match validate_jwt(token_value) {
                Ok(claims) => {
                    Ok(Json(json!({
                        "success": true,
                        "authenticated": true,
                        "user_id": claims.user_id,
                        "display_name": claims.display_name,
                        "canvas_permissions": claims.device_permissions
                    })))
                }
                Err(_) => {
                    // Invalid token, return guest user
                    Ok(Json(json!({
                        "success": true,
                        "authenticated": false,
                        "user_id": "guest",
                        "display_name": "Guest User",
                        "canvas_permissions": {}
                    })))
                }
            }
        }
        None => {
            // No token, return guest user
            Ok(Json(json!({
                "success": true,
                "authenticated": false,
                "user_id": "guest",
                "display_name": "Guest User",
                "canvas_permissions": {}
            })))
        }
    }
}

// POST /api/profile/display-name - Change display name
// Website feature: Allows users to change their display name
async fn update_display_name_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    Json(req): Json<UpdateDisplayNameRequest>,
) -> Result<Response<Body>, StatusCode> {
    // Extract JWT token from cookie
    let token = match cookie_jar.get("auth_token") {
        Some(cookie) => cookie.value(),
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Validate JWT and extract claims
    let claims = match validate_jwt(token) {
        Ok(claims) => claims,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Validate display name (not empty, max 50 characters)
    if req.display_name.trim().is_empty() || req.display_name.len() > 50 {
        let response = AuthResponse {
            success: false,
            message: "Display name must be between 1 and 50 characters".to_string(),
            email: None,
        };
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&response).unwrap()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Update display name in database
    if let Err(e) = app_state.db.update_user_display_name(&claims.user_id, req.display_name.trim()).await {
        tracing::error!("Database error during display name update: {:?}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!("Display name updated for user: {}", claims.email);

    // Load updated user from database
    let updated_db_user = match app_state.db.get_user_by_id(&claims.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            tracing::error!("User {} not found in database after update", claims.user_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Database error loading updated user: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert user for JWT
    let user = User {
        id: updated_db_user.id.clone(),
        email: updated_db_user.email.clone(),
        display_name: updated_db_user.display_name.clone(),
        password_hash: updated_db_user.password_hash.clone(),
    };

    // Create new JWT with updated display name
    match create_jwt(&user) {
        Ok(new_token) => {
            let response = AuthResponse {
                success: true,
                message: "Display name updated successfully".to_string(),
                email: Some(claims.email),
            };

            Response::builder()
                .header("set-cookie", create_auth_cookie(&new_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(e) => {
            tracing::error!("JWT creation failed: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ============================================================================
// A 5.4: CANVAS MANAGEMENT HANDLERS - API for canvas management with permissions
// ============================================================================

// GET /api/devices - List all devices (optional auth)
async fn list_devices_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
) -> Result<Json<Value>, StatusCode> {
    // Validate JWT token (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    let user_id = match token {
        Some(token_value) => {
            match validate_jwt(token_value) {
                Ok(claims) => Some(claims.user_id),
                Err(_) => None,
            }
        }
        None => None,
    };

    // Load devices from database
    let device_list = match &user_id {
        Some(uid) => {
            // If authenticated, show user's devices with permissions
            match app_state.db.list_user_devices(uid).await {
                Ok(device_list) => {
                    device_list.into_iter().map(|(device, permission)| {
                        json!({
                            "id": device.mac_address.clone(),
                            "name": device.name,
                            "mac_address": device.mac_address.replace('-', ":"),  // Show with colons for display
                            "ip_address": device.ip_address,
                            "status": device.status,
                            "maintenance_mode": device.maintenance_mode,
                            "firmware_version": device.firmware_version,
                            "owner_id": device.owner_id,
                            "last_seen": device.last_seen.to_rfc3339(),
                            "created_at": device.created_at.to_rfc3339(),
                            "your_permission": permission
                        })
                    }).collect::<Vec<Value>>()
                }
                Err(e) => {
                    tracing::error!("Database error during device list: {:?}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
        None => {
            // If not authenticated, show all devices with guest permission
            match app_state.db.list_all_devices().await {
                Ok(device_list) => {
                    device_list.into_iter().map(|device| {
                        json!({
                            "id": device.mac_address.clone(),
                            "name": device.name,
                            "mac_address": device.mac_address.replace('-', ":"),  // Show with colons for display
                            "ip_address": device.ip_address,
                            "status": device.status,
                            "maintenance_mode": device.maintenance_mode,
                            "firmware_version": device.firmware_version,
                            "owner_id": device.owner_id,
                            "last_seen": device.last_seen.to_rfc3339(),
                            "created_at": device.created_at.to_rfc3339(),
                            "your_permission": "GUEST"
                        })
                    }).collect::<Vec<Value>>()
                }
                Err(e) => {
                    tracing::error!("Database error during device list: {:?}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
    };

    Ok(Json(json!({
        "success": true,
        "devices": device_list
    })))
}

// POST /api/devices - Create new ESP32 device (optional auth)
async fn create_device_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    Json(req): Json<CreateDeviceRequest>,
) -> Result<Response<Body>, StatusCode> {
    // Validate JWT token (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    let owner_id = match token {
        Some(token_value) => {
            match validate_jwt(token_value) {
                Ok(claims) => claims.user_id,
                Err(_) => "guest".to_string(),
            }
        }
        None => "guest".to_string(),
    };

    // Validate device name and MAC address
    if req.name.trim().is_empty() || req.name.len() > 100 {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(json!({"success": false, "message": "Device name must be between 1 and 100 characters"}).to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    if req.mac_address.trim().is_empty() {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(json!({"success": false, "message": "MAC address is required"}).to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Convert MAC address to key format (replace : with -)
    let mac_key = req.mac_address.trim().replace(':', "-");

    // Create new ESP32 device
    let device = database::ESP32Device::new(
        req.name.trim().to_string(),
        owner_id.clone(),
        mac_key,
    );

    // Save device to database
    if let Err(e) = app_state.db.create_esp32_device(device.clone()).await {
        tracing::error!("Database error during device creation: {:?}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let user_info = if owner_id == "guest" { "guest user".to_string() } else { owner_id.clone() };
    tracing::info!("ESP32 device created: {} by user {}", device.name, user_info);

    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(json!({
            "success": true,
            "message": "Canvas created successfully",
            "device": {
                "id": device.mac_address.clone(),
                "name": device.name,
                "mac_address": device.mac_address.replace('-', ":"),  // Show with colons for display
                "ip_address": device.ip_address,
                "status": device.status,
                "maintenance_mode": device.maintenance_mode,
                "firmware_version": device.firmware_version,
                "owner_id": device.owner_id,
                "last_seen": device.last_seen.to_rfc3339(),
                "created_at": device.created_at.to_rfc3339(),
                "your_permission": "O"
            }
        }).to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// GET /api/devices/:id - Details of an ESP32 device (optional auth)
async fn get_device_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    Path(canvas_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    // JWT Token validieren (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    let user_id = match token {
        Some(token_value) => {
            match validate_jwt(token_value) {
                Ok(claims) => Some(claims.user_id),
                Err(_) => None,
            }
        }
        None => None,
    };

    // Canvas aus Datenbank laden
    let canvas = match app_state.db.get_esp32_device_by_id(&canvas_id).await {
        Ok(Some(canvas)) => canvas,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Database error loading canvas: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // User-Berechtigung laden (falls authenticated)
    let user_permission = match &user_id {
        Some(uid) => {
            match app_state.db.get_user_device_permission(&canvas_id, uid).await {
                Ok(Some(permission)) => permission,
                Ok(None) => "NONE".to_string(),
                Err(e) => {
                    tracing::error!("Database error loading user permission: {:?}", e);
                    "NONE".to_string()
                }
            }
        }
        None => "GUEST".to_string(), // Guest user has guest permission
    };

    // Load all permissions (only for moderators or guest gets all)
    let all_permissions = match &user_id {
        Some(uid) => {
            if app_state.db.user_has_device_permission(&canvas_id, uid, "M").await.unwrap_or(false) {
                Some(app_state.db.get_device_permissions(&canvas_id).await.unwrap_or_default())
            } else {
                None
            }
        }
        None => {
            // Guest users can see all permissions for transparency
            Some(app_state.db.get_device_permissions(&canvas_id).await.unwrap_or_default())
        }
    };

    Ok(Json(json!({
        "success": true,
        "canvas": {
            "id": canvas.mac_address,
            "name": canvas.name,
            "maintenance_mode": canvas.maintenance_mode,
            "owner_id": canvas.owner_id,
            "created_at": canvas.created_at.to_rfc3339(),
            "your_permission": user_permission,
            "all_permissions": all_permissions
        }
    })))
}

// POST /api/devices/:id - Device-Eigenschaften ändern (Name, Wartungsmodus) (optional auth)
async fn update_device_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    Path(canvas_id): Path<String>,
    Json(req): Json<UpdateDeviceRequest>,
) -> Result<Response<Body>, StatusCode> {
    // JWT Token validieren (optional)
    let token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    let user_email = match token {
        Some(token_value) => {
            match validate_jwt(token_value) {
                Ok(claims) => Some(claims.email),
                Err(_) => None,
            }
        }
        None => None,
    };

    // Canvas aus Datenbank laden
    let _canvas = match app_state.db.get_esp32_device_by_id(&canvas_id).await {
        Ok(Some(canvas)) => canvas,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Database error loading canvas: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Guest users have full permissions for device management
    // (Authentication is optional, so no permission checks needed)

    // Validate name if provided
    if let Some(name) = &req.name {
        if name.trim().is_empty() || name.len() > 100 {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(json!({"success": false, "message": "Canvas name must be between 1 and 100 characters"}).to_string()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Update canvas
    if let Err(e) = app_state.db.update_esp32_device(
        &canvas_id,
        req.name.as_ref().map(|s| s.trim()),
        req.maintenance_mode
    ).await {
        tracing::error!("Database error updating canvas: {:?}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Aktualisierte Canvas laden
    let updated_canvas = match app_state.db.get_esp32_device_by_id(&canvas_id).await {
        Ok(Some(canvas)) => canvas,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Database error loading updated canvas: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let user_info = user_email.unwrap_or_else(|| "guest".to_string());
    tracing::info!("Canvas updated: {} by user {}", updated_canvas.name, user_info);

    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(json!({
            "success": true,
            "message": "Canvas updated successfully",
            "canvas": {
                "id": updated_canvas.mac_address.clone(),
                "name": updated_canvas.name,
                "maintenance_mode": updated_canvas.maintenance_mode,
                "owner_id": updated_canvas.owner_id,
                "created_at": updated_canvas.created_at.to_rfc3339(),
                "mac_address": updated_canvas.mac_address.replace('-', ":")  // Show with colons for display
            }
        }).to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}


// POST /api/canvas-permissions/:id - Vereinfachter Permission Handler (optional auth)
async fn simple_permissions_handler(
    State(app_state): State<AppState>,
    Path(canvas_id): Path<String>,
    cookie_jar: CookieJar,
    Json(req): Json<UpdatePermissionRequest>,
) -> Result<Json<Value>, StatusCode> {
    // JWT Token validieren (optional)
    let _token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    // Authentication is optional, so no validation needed

    // Validate permission
    if req.permission != "REMOVE" && !["R", "W", "V", "M", "O"].contains(&req.permission.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Update or remove permission
    let result = if req.permission == "REMOVE" {
        app_state.db.remove_device_permission(&canvas_id, &req.user_id).await
    } else {
        app_state.db.set_device_permission(&canvas_id, &req.user_id, &req.permission).await
    };

    if result.is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Permission updated successfully"
    })))
}

// DELETE /api/devices/:id - ESP32 Device löschen
async fn delete_device_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    Path(canvas_id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    // JWT Token validieren
    let token = match cookie_jar.get("auth_token") {
        Some(cookie) => cookie.value(),
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let claims = match validate_jwt(token) {
        Ok(claims) => claims,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Canvas aus Datenbank laden
    let canvas = match app_state.db.get_esp32_device_by_id(&canvas_id).await {
        Ok(Some(canvas)) => canvas,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Database error loading canvas: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Only owner can delete canvas
    let has_permission = match app_state.db.user_has_device_permission(&canvas_id, &claims.user_id, "O").await {
        Ok(has_permission) => has_permission,
        Err(e) => {
            tracing::error!("Database error checking permissions: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if !has_permission {
        return Err(StatusCode::FORBIDDEN);
    }

    // Canvas löschen
    if let Err(e) = app_state.db.delete_esp32_device(&canvas_id).await {
        tracing::error!("Database error deleting canvas: {:?}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!("Canvas deleted: {} by user {}", canvas.name, claims.email);

    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(json!({
            "success": true,
            "message": "Canvas deleted successfully"
        }).to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// GET /api/users/search - Search for users for permission management (optional auth)
async fn search_users_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    // Validate JWT token (optional)
    let _token = cookie_jar.get("auth_token").map(|cookie| cookie.value());
    
    // Authentication is optional, so no validation needed

    // Suchterm aus Query-Parameter extrahieren
    let query = params.get("q").cloned().unwrap_or_default();
    
    if query.len() < 2 {
        return Ok(Json(json!({
            "success": true,
            "users": []
        })));
    }

    // Benutzer in Datenbank suchen
    let matching_users = match app_state.db.search_users(&query).await {
        Ok(users) => {
            users.into_iter().map(|user| {
                json!({
                    "user_id": user.id,
                    "display_name": user.display_name
                })
            }).collect::<Vec<Value>>()
        }
        Err(e) => {
            tracing::error!("Database error during user search: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(json!({
        "success": true,
        "users": matching_users
    })))
}

// GET /api/users/list - Get first users for scroll field  
async fn list_users_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    // Validate JWT token
    let token = match cookie_jar.get("auth_token") {
        Some(cookie) => cookie.value(),
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let _claims = match validate_jwt(token) {
        Ok(claims) => claims,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Pagination parameters 
    let offset = params.get("offset").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let limit = params.get("limit").and_then(|s| s.parse::<i32>().ok()).unwrap_or(20);
    let limit = limit.min(50); // Max 50 users at once

    // Get users from database
    let users = match app_state.db.get_users_paginated(offset, limit).await {
        Ok(users) => {
            users.into_iter().map(|user| {
                json!({
                    "user_id": user.id,
                    "display_name": user.display_name
                })
            }).collect::<Vec<Value>>()
        }
        Err(e) => {
            tracing::error!("Database error during user list: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(json!({
        "success": true,
        "users": users
    })))
}

// GET /api/esp32/discovered - List discovered ESP32 devices (authentication optional)
async fn discovered_esp32_devices_handler(
    State(app_state): State<AppState>,
    cookie_jar: CookieJar,
) -> Result<Json<Value>, StatusCode> {
    // Extract JWT token from cookie (optional)
    let _token = cookie_jar.get("auth_token").map(|cookie| cookie.value());

    // Authentication is optional for ESP32 device discovery
    // Get discovered devices from ESP32Discovery service (TCP/mDNS devices)
    let discovered_devices = {
        let discovery = app_state.esp32_discovery.lock().await;
        discovery.get_discovered_devices().await
    };

    tracing::info!("ESP32 Discovery API called - found {} TCP devices", discovered_devices.len());

    let mut devices_json: Vec<Value> = discovered_devices
        .into_iter()
        .map(|(device_id, discovered_device)| {
            tracing::info!("Processing discovered device: {} with mDNS data: {:?}",
                device_id, discovered_device.mdns_data.is_some());

            let mut device_json = json!({
                "deviceId": device_id,
                "deviceIp": discovered_device.device_config.ip_address.to_string(),
                "tcpPort": discovered_device.device_config.tcp_port,
                "udpPort": discovered_device.device_config.udp_port,
                "status": "discovered",
                "connectionType": "tcp"
            });

            // Add MAC address and mDNS hostname from mDNS data if available
            if let Some(ref mdns_data) = discovered_device.mdns_data {
                tracing::info!("Found mDNS data with {} TXT records", mdns_data.txt_records.len());
                if let Some(mac_address) = mdns_data.txt_records.get("mac") {
                    tracing::info!("Adding MAC address to JSON: {}", mac_address);
                    device_json["macAddress"] = json!(mac_address);
                } else {
                    tracing::warn!("No 'mac' key found in TXT records: {:?}", mdns_data.txt_records.keys().collect::<Vec<_>>());
                }

                // Add mDNS hostname without .local suffix
                let mdns_hostname = mdns_data.hostname.replace(".local", "").trim_end_matches('.').to_string();
                device_json["mdnsHostname"] = json!(mdns_hostname);
                tracing::info!("Adding mDNS hostname to JSON: {}", mdns_hostname);
            } else {
                tracing::warn!("No mDNS data found for device: {}", device_id);
            }

            device_json
        })
        .collect();

    // Also get UART devices from device store events
    let uart_devices = app_state.device_store.get_device_events("system").await;
    let mut uart_device_ids = std::collections::HashSet::new();

    for event in uart_devices {
        if let crate::events::DeviceEvent::Esp32DeviceDiscovered {
            device_id,
            device_ip,
            mdns_hostname,
            ..
        } = event {
            // Only add UART devices (IP 0.0.0.0)
            if device_ip == "0.0.0.0" && !uart_device_ids.contains(&device_id) {
                uart_device_ids.insert(device_id.clone());

                devices_json.push(json!({
                    "deviceId": device_id,
                    "deviceIp": device_ip,
                    "tcpPort": 0,
                    "udpPort": 0,
                    "status": "discovered",
                    "connectionType": "uart",
                    "mdnsHostname": mdns_hostname
                }));

                tracing::info!("Added UART device to discovered list: {}", device_id);
            }
        }
    }

    tracing::info!("Total discovered devices: {} (TCP: {}, UART: {})",
                   devices_json.len(), devices_json.len() - uart_device_ids.len(), uart_device_ids.len());

    Ok(Json(json!({
        "success": true,
        "devices": devices_json
    })))
}

// ============================================================================
// UART SETTINGS HANDLERS - API handlers for UART configuration
// ============================================================================

// GET /api/uart/settings - Get current UART settings
async fn get_uart_settings_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    match app_state.db.get_uart_settings().await {
        Ok(Some((port, baud_rate, auto_connect))) => {
            Ok(Json(json!({
                "success": true,
                "port": port,
                "baudRate": baud_rate,
                "autoConnect": auto_connect
            })))
        }
        Ok(None) => {
            Ok(Json(json!({
                "success": true,
                "port": null,
                "baudRate": 115200,
                "autoConnect": false
            })))
        }
        Err(e) => {
            tracing::error!("Failed to get UART settings: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// POST /api/uart/settings - Update UART settings
#[derive(Debug, Deserialize)]
struct UpdateUartSettingsRequest {
    port: Option<String>,
    #[serde(rename = "baudRate")]
    baud_rate: u32,
    #[serde(rename = "autoConnect")]
    auto_connect: bool,
}

// POST /api/debug/settings - Update debug settings
#[derive(Debug, Deserialize)]
struct UpdateDebugSettingsRequest {
    #[serde(rename = "maxDebugMessages")]
    max_debug_messages: u32,
}

async fn update_uart_settings_handler(
    State(app_state): State<AppState>,
    Json(req): Json<UpdateUartSettingsRequest>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("Updating UART settings: port={:?}, baud_rate={}, auto_connect={}",
        req.port, req.baud_rate, req.auto_connect);

    match app_state.db.update_uart_settings(
        req.port.as_deref(),
        req.baud_rate,
        req.auto_connect
    ).await {
        Ok(()) => {
            Ok(Json(json!({
                "success": true,
                "message": "UART settings updated successfully"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to update UART settings: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// GET /api/uart/ports - List available serial ports
async fn list_uart_ports_handler() -> Result<Json<Value>, StatusCode> {
    match uart_connection::UartConnection::list_ports() {
        Ok(ports) => {
            Ok(Json(json!({
                "success": true,
                "ports": ports
            })))
        }
        Err(e) => {
            tracing::error!("Failed to list UART ports: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// POST /api/uart/connect - Connect to UART port
#[derive(Debug, Deserialize)]
struct UartConnectRequest {
    port: String,
    #[serde(rename = "baudRate")]
    baud_rate: u32,
    #[serde(rename = "autoConnect", default)]
    auto_connect: bool,
}

async fn uart_connect_handler(
    State(app_state): State<AppState>,
    Json(req): Json<UartConnectRequest>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("UART connect request: port={}, baud_rate={}, auto_connect={}",
        req.port, req.baud_rate, req.auto_connect);

    // Try to connect first
    let mut uart = app_state.uart_connection.lock().await;
    match uart.connect(req.port.clone(), req.baud_rate).await {
        Ok(()) => {
            drop(uart); // Release lock before database operation

            // Save settings to database after successful connection
            if let Err(e) = app_state.db.update_uart_settings(
                Some(&req.port),
                req.baud_rate,
                req.auto_connect
            ).await {
                tracing::error!("Failed to save UART settings after connect: {}", e);
                // Don't fail the connection just because settings save failed
            } else {
                tracing::info!("UART settings saved to database");
            }

            Ok(Json(json!({
                "success": true,
                "message": format!("Connected to UART port {} and settings saved", req.port)
            })))
        }
        Err(e) => {
            tracing::error!("Failed to connect to UART port: {}", e);
            Ok(Json(json!({
                "success": false,
                "message": format!("Failed to connect: {}", e)
            })))
        }
    }
}

// POST /api/uart/disconnect - Disconnect from UART port
async fn uart_disconnect_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("UART disconnect request");

    let mut uart = app_state.uart_connection.lock().await;
    match uart.disconnect().await {
        Ok(()) => {
            Ok(Json(json!({
                "success": true,
                "message": "Disconnected from UART port"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to disconnect from UART port: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// GET /api/uart/status - Get UART connection status
async fn uart_status_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let uart = app_state.uart_connection.lock().await;
    let is_connected = uart.is_connected().await;
    let settings = uart.get_settings().await;

    Ok(Json(json!({
        "success": true,
        "connected": is_connected,
        "port": settings.as_ref().map(|s| &s.port),
        "baudRate": settings.map(|s| s.baud_rate).unwrap_or(115200)
    })))
}

// ============================================================================
// DEBUG SETTINGS HANDLERS - API handlers for debug configuration
// ============================================================================

// GET /api/debug/settings - Get current debug settings
async fn get_debug_settings_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    match app_state.db.get_debug_settings().await {
        Ok(Some(max_debug_messages)) => {
            Ok(Json(json!({
                "success": true,
                "maxDebugMessages": max_debug_messages
            })))
        }
        Ok(None) => {
            Ok(Json(json!({
                "success": true,
                "maxDebugMessages": 200
            })))
        }
        Err(e) => {
            tracing::error!("Failed to get debug settings: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn update_debug_settings_handler(
    State(app_state): State<AppState>,
    Json(req): Json<UpdateDebugSettingsRequest>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("Updating debug settings: max_debug_messages={}", req.max_debug_messages);

    // Validate: min 10, max 10000
    if req.max_debug_messages < 10 || req.max_debug_messages > 10000 {
        return Ok(Json(json!({
            "success": false,
            "message": "Max debug messages must be between 10 and 10000"
        })));
    }

    // Update database
    match app_state.db.update_debug_settings(req.max_debug_messages).await {
        Ok(()) => {}
        Err(e) => {
            tracing::error!("Failed to update debug settings: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Update device store limit immediately
    app_state.device_store.set_max_debug_messages(req.max_debug_messages as usize).await;
    tracing::info!("Debug settings updated in database and device store");

    Ok(Json(json!({
        "success": true,
        "message": "Debug settings updated successfully"
    })))
}