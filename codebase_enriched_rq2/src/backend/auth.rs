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




