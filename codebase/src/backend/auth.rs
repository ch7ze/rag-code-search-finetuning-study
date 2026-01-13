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


// JWT token creation and validation
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

// Validates a JWT token and returns the claims
// Website feature: Checks if a user is still logged in
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

// Erstellt ein sicheres Auth-Cookie mit JWT Token
// Website-Feature: Wird nach erfolgreichem Login gesetzt
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

// Erstellt ein Logout-Cookie (löscht das Auth-Cookie)
// Website-Feature: Wird beim Logout aufgerufen
pub fn create_logout_cookie() -> HeaderValue {
    // Max-Age=0 = Cookie sofort löschen
    let cookie_value = "auth_token=; HttpOnly; Path=/; Max-Age=0; SameSite=Strict";
    HeaderValue::from_str(cookie_value).unwrap()
}

// ============================================================================
// ESP32 DEVICE MANAGEMENT - Funktionen für ESP32-Verwaltung und Berechtigungen
// Website-Feature: A 5.4 Rechtesystem Implementation adapted for ESP32
// ============================================================================




