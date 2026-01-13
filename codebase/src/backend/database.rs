// ============================================================================
// DATABASE MODULE - SQLite Datenbankintegration für User-Management & ESP32-Device-Management
// ============================================================================

use sqlx::{sqlite::SqlitePool, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use bcrypt::{hash, verify, DEFAULT_COST};
use serde::{Deserialize, Serialize};
use std::fs;

// ============================================================================
// DATABASE STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct InitialUserConfig {
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitialUsersFile {
    pub users: Vec<InitialUserConfig>,
}

#[derive(Debug, Clone)]
pub struct DatabaseUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ESP32Device {
    pub mac_address: String, // Primary key - moved to first position
    pub name: String,
    pub owner_id: String,
    pub ip_address: Option<String>,
    pub status: DeviceStatus,
    pub maintenance_mode: bool,
    pub firmware_version: Option<String>,
    pub last_seen: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceStatus {
    Online,
    Offline,
    Error,
    Updating,
    Maintenance,
}

#[derive(Debug, Clone, Serialize)]
pub struct ESP32DevicePermission {
    pub device_id: String,
    pub user_id: String,
    pub permission: String,
}

impl DatabaseUser {
    pub fn new(email: String, display_name: String, password: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let password_hash = hash(password, DEFAULT_COST)?;
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            email,
            display_name,
            password_hash,
            created_at: Utc::now(),
            is_admin: false,
        })
    }

    pub fn verify_password(&self, password: &str) -> Result<bool, bcrypt::BcryptError> {
        verify(password, &self.password_hash)
    }
}

impl ESP32Device {
    pub fn new(name: String, owner_id: String, mac_address: String) -> Self {
        let now = Utc::now();
        Self {
            mac_address, // Primary key
            name,
            owner_id,
            ip_address: None,
            status: DeviceStatus::Offline,
            maintenance_mode: false,
            firmware_version: None,
            last_seen: now,
            created_at: now,
        }
    }

    pub fn update_status(&mut self, status: DeviceStatus, ip_address: Option<String>) {
        self.status = status;
        self.ip_address = ip_address;
        self.last_seen = Utc::now();
    }
}

// ============================================================================
// DATABASE MANAGER
// ============================================================================

#[derive(Debug)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Erstelle SQLite-Datenbankdatei wenn sie nicht existiert
        std::fs::create_dir_all("data").ok();
        
        let database_url = "sqlite:data/users.db?mode=rwc";
        let pool = SqlitePool::connect(database_url).await?;
        
        let db_manager = Self { pool };
        
        // Tabellen erstellen
        db_manager.init_database().await?;
        
        // Initiale User aus Konfiguration erstellen
        db_manager.create_initial_users().await?;
        
        Ok(db_manager)
    }

    async fn init_database(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Users Tabelle erstellen
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                display_name TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                is_admin BOOLEAN NOT NULL DEFAULT FALSE
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // ESP32 Devices Tabelle erstellen
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS esp32_devices (
                mac_address TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                ip_address TEXT,
                status TEXT NOT NULL DEFAULT 'Offline',
                maintenance_mode BOOLEAN NOT NULL DEFAULT FALSE,
                firmware_version TEXT,
                last_seen TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (owner_id) REFERENCES users (id)
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // ESP32 Device Permissions Tabelle erstellen
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS esp32_device_permissions (
                device_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                permission TEXT NOT NULL,
                PRIMARY KEY (device_id, user_id),
                FOREIGN KEY (device_id) REFERENCES esp32_devices (mac_address),
                FOREIGN KEY (user_id) REFERENCES users (id)
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // UART Settings Tabelle erstellen
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS uart_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                port TEXT,
                baud_rate INTEGER NOT NULL DEFAULT 115200,
                auto_connect BOOLEAN NOT NULL DEFAULT FALSE,
                updated_at TEXT NOT NULL
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // Insert default UART settings if not exists
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO uart_settings (id, port, baud_rate, auto_connect, updated_at)
            VALUES (1, NULL, 115200, FALSE, datetime('now'))
            "#
        )
        .execute(&self.pool)
        .await?;

        // Debug Settings Tabelle erstellen
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS debug_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                max_debug_messages INTEGER NOT NULL DEFAULT 200,
                updated_at TEXT NOT NULL
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // Insert default Debug settings if not exists
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO debug_settings (id, max_debug_messages, updated_at)
            VALUES (1, 200, datetime('now'))
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn create_user(&self, user: DatabaseUser) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, created_at, is_admin) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(user.created_at.to_rfc3339())
        .bind(user.is_admin)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<DatabaseUser>, Box<dyn std::error::Error>> {
        let row = sqlx::query("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let created_at_str: String = row.get("created_at");
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
                
                Ok(Some(DatabaseUser {
                    id: row.get("id"),
                    email: row.get("email"),
                    display_name: row.get("display_name"),
                    password_hash: row.get("password_hash"),
                    created_at,
                    is_admin: row.get("is_admin"),
                }))
            }
            None => Ok(None)
        }
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<DatabaseUser>, Box<dyn std::error::Error>> {
        let row = sqlx::query("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let created_at_str: String = row.get("created_at");
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
                
                Ok(Some(DatabaseUser {
                    id: row.get("id"),
                    email: row.get("email"),
                    display_name: row.get("display_name"),
                    password_hash: row.get("password_hash"),
                    created_at,
                    is_admin: row.get("is_admin"),
                }))
            }
            None => Ok(None)
        }
    }

    pub async fn update_user_display_name(&self, user_id: &str, display_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?")
            .bind(display_name)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_all_users(&self) -> Result<Vec<DatabaseUser>, Box<dyn std::error::Error>> {
        let rows = sqlx::query("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;

        let mut users = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
            
            users.push(DatabaseUser {
                id: row.get("id"),
                email: row.get("email"),
                display_name: row.get("display_name"),
                password_hash: row.get("password_hash"),
                created_at,
                is_admin: row.get("is_admin"),
            });
        }

        Ok(users)
    }

    pub async fn search_users(&self, query: &str) -> Result<Vec<DatabaseUser>, Box<dyn std::error::Error>> {
        let search_pattern = format!("%{}%", query);
        let rows = sqlx::query("SELECT * FROM users WHERE email LIKE ? OR display_name LIKE ? ORDER BY display_name LIMIT 20")
            .bind(&search_pattern)
            .bind(&search_pattern)
            .fetch_all(&self.pool)
            .await?;

        let mut users = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
            
            users.push(DatabaseUser {
                id: row.get("id"),
                email: row.get("email"),
                display_name: row.get("display_name"),
                password_hash: row.get("password_hash"),
                created_at,
                is_admin: row.get("is_admin"),
            });
        }

        Ok(users)
    }

    pub async fn get_users_paginated(&self, offset: i32, limit: i32) -> Result<Vec<DatabaseUser>, Box<dyn std::error::Error>> {
        let rows = sqlx::query("SELECT * FROM users ORDER BY display_name LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let mut users = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
            
            users.push(DatabaseUser {
                id: row.get("id"),
                email: row.get("email"),
                display_name: row.get("display_name"),
                password_hash: row.get("password_hash"),
                created_at,
                is_admin: row.get("is_admin"),
            });
        }

        Ok(users)
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Zuerst Canvas Permissions löschen
        sqlx::query("DELETE FROM esp32_device_permissions WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        // Dann User löschen
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_user_admin_status(&self, user_id: &str, is_admin: bool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
            .bind(is_admin)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // ============================================================================
    // INITIAL USERS MANAGEMENT - Lädt und erstellt initiale User aus Konfiguration
    // ============================================================================

    fn load_initial_users() -> Result<InitialUsersFile, Box<dyn std::error::Error>> {
        let config_path = "data/initial_users.json";
        
        if !std::path::Path::new(config_path).exists() {
            tracing::warn!("Initial users config file not found: {}", config_path);
            // Fallback zu Standard Admin-User
            return Ok(InitialUsersFile {
                users: vec![InitialUserConfig {
                    email: "admin@drawing-app.local".to_string(),
                    display_name: "Administrator".to_string(),
                    password: "admin123".to_string(),
                    is_admin: true,
                }],
            });
        }

        let config_content = fs::read_to_string(config_path)?;
        let config: InitialUsersFile = serde_json::from_str(&config_content)?;
        
        tracing::info!("Loaded {} initial users from config", config.users.len());
        Ok(config)
    }

    async fn create_initial_users(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Prüfen ob bereits User existieren
        let user_count = sqlx::query("SELECT COUNT(*) as count FROM users")
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>("count");

        if user_count > 0 {
            tracing::info!("Database contains {} existing users, skipping initial user creation", user_count);
            return Ok(());
        }

        // Initiale User aus Konfiguration laden
        let config = Self::load_initial_users()?;
        let mut created_count = 0;

        for user_config in config.users {
            tracing::debug!("Creating initial user: {}", user_config.email);
            
            let db_user = DatabaseUser {
                id: Uuid::new_v4().to_string(),
                email: user_config.email.clone(),
                display_name: user_config.display_name,
                password_hash: hash(&user_config.password, DEFAULT_COST)?,
                created_at: Utc::now(),
                is_admin: user_config.is_admin,
            };

            match self.create_user(db_user).await {
                Ok(_) => {
                    created_count += 1;
                    if user_config.is_admin {
                        tracing::info!("Created initial admin user: {} / {}", user_config.email, user_config.password);
                    } else {
                        tracing::info!("Created initial user: {} / {}", user_config.email, user_config.password);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to create initial user {}: {:?}", user_config.email, e);
                }
            }
        }

        if created_count > 0 {
            tracing::info!("Successfully created {} initial users", created_count);
        }

        Ok(())
    }

    // ============================================================================
    // ESP32 DEVICE MANAGEMENT - CRUD Operationen für ESP32 Devices
    // ============================================================================

    pub async fn create_esp32_device(&self, device: ESP32Device) -> Result<(), Box<dyn std::error::Error>> {
        let status_str = match device.status {
            DeviceStatus::Online => "Online",
            DeviceStatus::Offline => "Offline", 
            DeviceStatus::Error => "Error",
            DeviceStatus::Updating => "Updating",
            DeviceStatus::Maintenance => "Maintenance",
        };
        
        sqlx::query(
            "INSERT INTO esp32_devices (mac_address, name, owner_id, ip_address, status, maintenance_mode, firmware_version, last_seen, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&device.mac_address)
        .bind(&device.name)
        .bind(&device.owner_id)
        .bind(&device.ip_address)
        .bind(status_str)
        .bind(device.maintenance_mode)
        .bind(&device.firmware_version)
        .bind(device.last_seen.to_rfc3339())
        .bind(device.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        // Owner-Berechtigung hinzufügen
        self.set_device_permission(&device.mac_address, &device.owner_id, "O").await?;

        Ok(())
    }

    pub async fn get_esp32_device_by_id(&self, device_id: &str) -> Result<Option<ESP32Device>, Box<dyn std::error::Error>> {
        let row = sqlx::query("SELECT * FROM esp32_devices WHERE mac_address = ?")
            .bind(device_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let created_at_str: String = row.get("created_at");
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
                let last_seen_str: String = row.get("last_seen");
                let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)?.with_timezone(&Utc);
                
                let status_str: String = row.get("status");
                let status = match status_str.as_str() {
                    "Online" => DeviceStatus::Online,
                    "Offline" => DeviceStatus::Offline,
                    "Error" => DeviceStatus::Error,
                    "Updating" => DeviceStatus::Updating,
                    "Maintenance" => DeviceStatus::Maintenance,
                    _ => DeviceStatus::Offline,
                };
                
                Ok(Some(ESP32Device {
                    mac_address: row.get("mac_address"),
                    name: row.get("name"),
                    owner_id: row.get("owner_id"),
                    ip_address: row.get("ip_address"),
                    status,
                    maintenance_mode: row.get("maintenance_mode"),
                    firmware_version: row.get("firmware_version"),
                    last_seen,
                    created_at,
                }))
            }
            None => Ok(None)
        }
    }

    pub async fn list_user_devices(&self, user_id: &str) -> Result<Vec<(ESP32Device, String)>, Box<dyn std::error::Error>> {
        let rows = sqlx::query(
            r#"
            SELECT d.*, dp.permission
            FROM esp32_devices d
            INNER JOIN esp32_device_permissions dp ON d.mac_address = dp.device_id
            WHERE dp.user_id = ?
            ORDER BY d.created_at DESC
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        let mut device_list = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
            let last_seen_str: String = row.get("last_seen");
            let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)?.with_timezone(&Utc);
            
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Online" => DeviceStatus::Online,
                "Offline" => DeviceStatus::Offline,
                "Error" => DeviceStatus::Error,
                "Updating" => DeviceStatus::Updating,
                "Maintenance" => DeviceStatus::Maintenance,
                _ => DeviceStatus::Offline,
            };
            
            let device = ESP32Device {
                mac_address: row.get("mac_address"),
                name: row.get("name"),
                owner_id: row.get("owner_id"),
                ip_address: row.get("ip_address"),
                status,
                maintenance_mode: row.get("maintenance_mode"),
                firmware_version: row.get("firmware_version"),
                last_seen,
                created_at,
            };
            
            let permission: String = row.get("permission");
            device_list.push((device, permission));
        }

        Ok(device_list)
    }

    pub async fn list_all_devices(&self) -> Result<Vec<ESP32Device>, Box<dyn std::error::Error>> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM esp32_devices
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut device_list = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
            let last_seen_str: String = row.get("last_seen");
            let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)?.with_timezone(&Utc);
            
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Online" => DeviceStatus::Online,
                "Offline" => DeviceStatus::Offline,
                "Error" => DeviceStatus::Error,
                "Updating" => DeviceStatus::Updating,
                "Maintenance" => DeviceStatus::Maintenance,
                _ => DeviceStatus::Offline,
            };
            
            let device = ESP32Device {
                mac_address: row.get("mac_address"),
                name: row.get("name"),
                owner_id: row.get("owner_id"),
                ip_address: row.get("ip_address"),
                status,
                maintenance_mode: row.get("maintenance_mode"),
                firmware_version: row.get("firmware_version"),
                last_seen,
                created_at,
            };
            
            device_list.push(device);
        }

        Ok(device_list)
    }

    pub async fn update_esp32_device(&self, device_id: &str, name: Option<&str>, maintenance_mode: Option<bool>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(name) = name {
            sqlx::query("UPDATE esp32_devices SET name = ? WHERE mac_address = ?")
                .bind(name)
                .bind(device_id)
                .execute(&self.pool)
                .await?;
        }

        if let Some(maintenance_mode) = maintenance_mode {
            sqlx::query("UPDATE esp32_devices SET maintenance_mode = ? WHERE mac_address = ?")
                .bind(maintenance_mode)
                .bind(device_id)
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    pub async fn update_device_status(&self, device_id: &str, status: &DeviceStatus, ip_address: Option<&str>, firmware_version: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let status_str = match status {
            DeviceStatus::Online => "Online",
            DeviceStatus::Offline => "Offline",
            DeviceStatus::Error => "Error", 
            DeviceStatus::Updating => "Updating",
            DeviceStatus::Maintenance => "Maintenance",
        };
        
        let now = Utc::now().to_rfc3339();
        
        sqlx::query("UPDATE esp32_devices SET status = ?, ip_address = ?, firmware_version = ?, last_seen = ? WHERE mac_address = ?")
            .bind(status_str)
            .bind(ip_address)
            .bind(firmware_version)
            .bind(now)
            .bind(device_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_esp32_device(&self, device_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Zuerst Berechtigungen löschen
        sqlx::query("DELETE FROM esp32_device_permissions WHERE device_id = ?")
            .bind(device_id)
            .execute(&self.pool)
            .await?;

        // Dann Device löschen
        sqlx::query("DELETE FROM esp32_devices WHERE mac_address = ?")
            .bind(device_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // ============================================================================
    // ESP32 DEVICE PERMISSIONS - Berechtigungsverwaltung
    // ============================================================================

    pub async fn set_device_permission(&self, device_id: &str, user_id: &str, permission: &str) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query(
            "INSERT OR REPLACE INTO esp32_device_permissions (device_id, user_id, permission) VALUES (?, ?, ?)"
        )
        .bind(device_id)
        .bind(user_id)
        .bind(permission)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn remove_device_permission(&self, device_id: &str, user_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("DELETE FROM esp32_device_permissions WHERE device_id = ? AND user_id = ?")
            .bind(device_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_device_permissions(&self, device_id: &str) -> Result<Vec<ESP32DevicePermission>, Box<dyn std::error::Error>> {
        let rows = sqlx::query("SELECT * FROM esp32_device_permissions WHERE device_id = ?")
            .bind(device_id)
            .fetch_all(&self.pool)
            .await?;

        let mut permissions = Vec::new();
        for row in rows {
            permissions.push(ESP32DevicePermission {
                device_id: row.get("device_id"),
                user_id: row.get("user_id"),
                permission: row.get("permission"),
            });
        }

        Ok(permissions)
    }

    pub async fn get_user_device_permission(&self, device_id: &str, user_id: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let row = sqlx::query("SELECT permission FROM esp32_device_permissions WHERE device_id = ? AND user_id = ?")
            .bind(device_id)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => Ok(Some(row.get("permission"))),
            None => Ok(None),
        }
    }

    pub async fn user_has_device_permission(&self, device_id: &str, user_id: &str, required_permission: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let user_permission = self.get_user_device_permission(device_id, user_id).await?;
        
        match user_permission {
            Some(permission) => {
                let has_permission = match required_permission {
                    "R" => ["R", "W", "V", "M", "O"].contains(&permission.as_str()),
                    "W" => {
                        // Prüfen ob Device im Wartungsmodus ist
                        let device = self.get_esp32_device_by_id(device_id).await?;
                        if let Some(device) = device {
                            if device.maintenance_mode {
                                ["V", "M", "O"].contains(&permission.as_str())
                            } else {
                                ["W", "V", "M", "O"].contains(&permission.as_str())
                            }
                        } else {
                            false
                        }
                    },
                    "V" => ["V", "M", "O"].contains(&permission.as_str()),
                    "M" => ["M", "O"].contains(&permission.as_str()),
                    "O" => permission == "O",
                    _ => false,
                };
                Ok(has_permission)
            }
            None => Ok(false),
        }
    }

    // ========================================================================
    // UART SETTINGS METHODS
    // ========================================================================

    /// Get UART settings from database
    pub async fn get_uart_settings(&self) -> Result<Option<(Option<String>, u32, bool)>, Box<dyn std::error::Error>> {
        let row = sqlx::query(
            "SELECT port, baud_rate, auto_connect FROM uart_settings WHERE id = 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let port: Option<String> = row.try_get("port")?;
                let baud_rate: i64 = row.try_get("baud_rate")?;
                let auto_connect: bool = row.try_get("auto_connect")?;
                Ok(Some((port, baud_rate as u32, auto_connect)))
            }
            None => Ok(None),
        }
    }

    /// Update UART settings in database
    pub async fn update_uart_settings(
        &self,
        port: Option<&str>,
        baud_rate: u32,
        auto_connect: bool
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query(
            r#"
            UPDATE uart_settings
            SET port = ?, baud_rate = ?, auto_connect = ?, updated_at = datetime('now')
            WHERE id = 1
            "#
        )
        .bind(port)
        .bind(baud_rate as i64)
        .bind(auto_connect)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ========================================================================
    // DEBUG SETTINGS METHODS
    // ========================================================================

    /// Get debug settings from database
    pub async fn get_debug_settings(&self) -> Result<Option<u32>, Box<dyn std::error::Error>> {
        let row = sqlx::query(
            "SELECT max_debug_messages FROM debug_settings WHERE id = 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let max_messages: i64 = row.try_get("max_debug_messages")?;
                Ok(Some(max_messages as u32))
            }
            None => Ok(None),
        }
    }

    /// Update debug settings in database
    pub async fn update_debug_settings(
        &self,
        max_debug_messages: u32
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query(
            r#"
            UPDATE debug_settings
            SET max_debug_messages = ?, updated_at = datetime('now')
            WHERE id = 1
            "#
        )
        .bind(max_debug_messages as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}