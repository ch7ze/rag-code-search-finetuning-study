// Authentication module for user management and ESP32 device management

use axum::http::HeaderValue;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// JWT secret key - should be loaded from environment variable in production
const JWT_SECRET: &[u8] = b"your-secret-key-should-be-much-longer-and-random";

// Data structures for authentication

// ESP32 Device representation with permissions and status  
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ESP32Device {
    pub id: String,
    pub name: String,
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub status: String,
    pub maintenance_mode: bool,
    pub owner_id: String,
    pub firmware_version: Option<String>,
    pub last_seen: String,
    pub created_at: String,
    pub permissions: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDeviceRequest {
    pub name: String,
    pub mac_address: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeviceRequest {
    pub name: Option<String>,
    pub maintenance_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePermissionRequest {
    pub user_id: String,
    pub permission: String,
}

// Registered user representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
}

// JWT token claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub device_permissions: HashMap<String, String>,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDisplayNameRequest {
    pub display_name: String,
}

// Response structure for authentication APIs
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub success: bool,
    pub message: String,
    pub email: Option<String>,
}


/// Creates a signed JWT authentication token for a user with device permissions.
///
/// Generates a JSON Web Token (JWT) containing user credentials (user_id, email, display_name)
/// and device-specific permissions for ESP32 access control. Token expires after 24 hours.
/// Uses HMAC-SHA256 signing with secret key for secure authentication.
///
/// # Use Cases
/// - User login authentication and session management
/// - ESP32 device access control with granular permissions (R/W/V/M/O)
/// - Stateless authentication for REST API endpoints
/// - WebSocket connection authorization
///
/// # Arguments
/// * `user` - User struct containing id, email, and display_name
///
/// # Returns
/// * `Ok(String)` - Signed JWT token string for HTTP cookies or Authorization headers
/// * `Err` - JWT encoding error if token creation fails
///
/// # Example Permissions
/// - "R" = Read-only access to device data
/// - "W" = Write access to control device
/// - "V" = View-only access (monitoring)
/// - "M" = Maintenance mode access
/// - "O" = Owner/admin permissions
///
/// Keywords: JWT token, authentication, user session, device permissions, ESP32 access control,
/// token generation, HMAC signing, stateless auth, REST API security
pub fn create_jwt(user: &User) -> Result<String, jsonwebtoken::errors::Error> {
    // Token expires after 24 hours
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .expect("valid timestamp")
        .timestamp() as usize;

    // Sample device permissions for demo purposes
    let mut device_permissions = HashMap::new();
    device_permissions.insert("esp32-abc123-def456-ghi789".to_string(), "R".to_string());
    device_permissions.insert("esp32-jkl012-mno345-pqr678".to_string(), "W".to_string());
    device_permissions.insert("esp32-stu901-vwx234-yza567".to_string(), "V".to_string());
    device_permissions.insert("esp32-bcd890-efg123-hij456".to_string(), "M".to_string());
    device_permissions.insert("esp32-klm789-nop012-qrs345".to_string(), "O".to_string());

    // Token claims
    let claims = Claims {
        user_id: user.id.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        device_permissions,
        exp: expiration,
    };

    // Create and sign the token
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET),
    )
}

// Create JWT with actual device permissions from store

/// Validates and decodes a JWT authentication token, verifying signature and expiration.
///
/// Decrypts and validates a JWT token string using the secret key and standard validation rules
/// (signature verification, expiration check). Returns the embedded claims (user_id, email,
/// display_name, device_permissions) if token is valid and not expired.
///
/// # Use Cases
/// - Verify user authentication status (check if logged in)
/// - Extract user identity from Authorization header or cookies
/// - Validate WebSocket connection authorization
/// - Middleware authentication for protected API routes
/// - Session validation before granting ESP32 device access
///
/// # Arguments
/// * `token` - JWT token string from Authorization header or auth_token cookie
///
/// # Returns
/// * `Ok(Claims)` - Decoded token claims with user info and device permissions
/// * `Err` - Invalid token, expired token, or signature verification failure
///
/// # Security
/// - Verifies HMAC-SHA256 signature to prevent token tampering
/// - Checks expiration timestamp (tokens valid for 24 hours)
/// - Rejects tokens with invalid format or missing claims
///
/// Keywords: JWT validation, token verification, authentication check, user session validation,
/// token decoding, signature verification, expired token, login status, stateless auth
pub fn validate_jwt(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    // Decrypt token and verify signature
    decode::<Claims>(
        token,                                    // JWT string
        &DecodingKey::from_secret(JWT_SECRET),   // Verification with secret
        &Validation::default(),                  // Standard validation (expiration date etc.)
    )
    .map(|data| data.claims)  // Only return claims, not the whole token
}

// ============================================================================
// PASSWORD SECURITY - Bcrypt hashing against brute-force attacks
// Website feature: Secure password storage
// ============================================================================


// ============================================================================
// USER IMPLEMENTATION - Methods for user objects
// Website feature: User creation and password verification
// ============================================================================

// impl block defines methods for the User struct
impl User {}


// ============================================================================
// COOKIE HELPER - Erstellt sichere HTTP-Cookies
// Website-Feature: Login-State im Browser speichern
// ============================================================================

/// Creates a secure HTTP-only authentication cookie containing the JWT token.
///
/// Generates a Set-Cookie header value with security flags (HttpOnly, SameSite=Strict) to store
/// the JWT authentication token in the user's browser. Cookie expires after 24 hours (86400 seconds).
/// Used after successful login to maintain user session state.
///
/// # Use Cases
/// - Store authentication token after user login
/// - Maintain session state in browser without JavaScript access
/// - Automatic authentication for subsequent HTTP requests
/// - Secure cookie-based authentication for web applications
///
/// # Arguments
/// * `token` - JWT token string generated by create_jwt()
///
/// # Returns
/// * `HeaderValue` - HTTP Set-Cookie header for axum response
///
/// # Security Features
/// - **HttpOnly**: Prevents JavaScript access to cookie (XSS protection)
/// - **SameSite=Strict**: Prevents CSRF attacks by blocking cross-site requests
/// - **Path=/**: Cookie valid for entire website domain
/// - **Max-Age=86400**: Cookie expires after 24 hours (matches JWT expiration)
///
/// Keywords: HTTP cookie, authentication cookie, secure cookie, HttpOnly flag, SameSite protection,
/// XSS prevention, CSRF protection, session cookie, login cookie, browser authentication
pub fn create_auth_cookie(token: &str) -> HeaderValue {
    let cookie_value = format!(
        "auth_token={}; HttpOnly; Path=/; Max-Age=86400; SameSite=Strict",
        token
    );
    // HttpOnly = JavaScript kann nicht auf Cookie zugreifen (XSS-Schutz)
    // Path=/ = Cookie gilt für ganze Website
    // Max-Age=86400 = Cookie läuft nach 24h ab (86400 Sekunden)
    // SameSite=Strict = Schutz vor CSRF-Attacken
    HeaderValue::from_str(&cookie_value).unwrap()
}

/// Creates a logout cookie that immediately deletes the authentication cookie.
///
/// Generates a Set-Cookie header with Max-Age=0 to instruct the browser to immediately delete
/// the auth_token cookie. Used during user logout to invalidate the session and clear authentication.
///
/// # Use Cases
/// - User logout functionality - clear session and authentication state
/// - Security: Force re-authentication after logout
/// - Session termination for web applications
/// - Clear cookies when switching between user accounts
///
/// # Returns
/// * `HeaderValue` - HTTP Set-Cookie header that expires the auth_token cookie
///
/// # How It Works
/// - Sets Max-Age=0 which tells browser to delete cookie immediately
/// - Maintains same security flags (HttpOnly, SameSite=Strict, Path=/)
/// - Empty cookie value ensures no residual authentication data
///
/// Keywords: logout cookie, delete cookie, clear authentication, session termination, cookie expiration,
/// logout functionality, invalidate session, clear auth token, browser logout
pub fn create_logout_cookie() -> HeaderValue {
    // Max-Age=0 = Cookie sofort löschen
    let cookie_value = "auth_token=; HttpOnly; Path=/; Max-Age=0; SameSite=Strict";
    HeaderValue::from_str(cookie_value).unwrap()
}

// ============================================================================
// ESP32 DEVICE MANAGEMENT - Funktionen für ESP32-Verwaltung und Berechtigungen
// Website-Feature: A 5.4 Rechtesystem Implementation adapted for ESP32
// ============================================================================




