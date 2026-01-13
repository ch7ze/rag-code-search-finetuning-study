use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use chrono::Utc;

pub struct DebugLogger;

impl DebugLogger {
    const LOG_DIR: &'static str = "logs";
    const LOG_FILE: &'static str = "logs/debug_events.log";
    const TCP_LOG_FILE: &'static str = "logs/tcp_messages.log";
    const TEMP_LOG_FILE: &'static str = "logs/templog.log";

    /// Creates logs directory if it doesn't exist.
    /// Ensures LOG_DIR exists before writing log files. Ignores errors if directory already exists.
    fn ensure_log_dir() {
        let _ = create_dir_all(Self::LOG_DIR);
    }

    /// Logs general debug event with category and message to debug_events.log file.
    /// Appends timestamped log entry in format "[timestamp] category: message" to logs/debug_events.log.
    /// Creates log directory and file if they don't exist. Flushes immediately to ensure write persistence.
    pub fn log_event(category: &str, message: &str) {
        Self::ensure_log_dir();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_entry = format!("[{}] {}: {}\n", timestamp, category, message);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::LOG_FILE) {
            let _ = file.write_all(log_entry.as_bytes());
            let _ = file.flush();
        }
    }

    /// Logs TCP communication messages to separate tcp_messages.log file with device ID and direction.
    /// Appends timestamped entry in format "[timestamp] TCP_direction: Device device_id - message" to logs/tcp_messages.log.
    /// Direction indicates IN (received) or OUT (sent). Used for debugging TCP communication with ESP32 devices.
    pub fn log_tcp_message(device_id: &str, direction: &str, message: &str) {
        Self::ensure_log_dir();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_entry = format!("[{}] TCP_{}: Device {} - {}\n", timestamp, direction, device_id, message);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::TCP_LOG_FILE) {
            let _ = file.write_all(log_entry.as_bytes());
            let _ = file.flush();
        }
    }

    /// Logs device addition event to debug log.
    /// Records when ADD_DEVICE is called for a specific device_id with DEVICE_MANAGEMENT category.
    pub fn log_device_add(device_id: &str) {
        Self::log_event("DEVICE_MANAGEMENT", &format!("ADD_DEVICE called for {}", device_id));
    }

    /// Logs when device addition fails because device already exists in the system.
    /// Records DEVICE_ALREADY_EXISTS event with device_id to prevent duplicate device registrations.
    pub fn log_device_already_exists(device_id: &str) {
        Self::log_event("DEVICE_MANAGEMENT", &format!("DEVICE_ALREADY_EXISTS {}", device_id));
    }

    /// Logs ESP32 connection event broadcast status with success/failure and channel state.
    /// Records whether event was sent successfully to listeners, if channel is closed, and any error details.
    /// Used for debugging WebSocket event broadcasting to connected clients for ESP32 device status changes.
    pub fn log_esp32_connection_event_send(device_id: &str, is_closed: bool, success: bool, error: Option<&str>) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        let details = if let Some(err) = error {
            format!(" error: {}", err)
        } else {
            String::new()
        };
        Self::log_event("ESP32_CONNECTION", &format!("EVENT_SEND {} for device {} (channel_closed: {}){}", status, device_id, is_closed, details));
    }

    /// Logs TCP command being sent to ESP32 device with availability status.
    /// Records command string, target device_id, and whether TCP connection is available before sending.
    pub fn log_tcp_command_send(device_id: &str, command: &str, tcp_available: bool) {
        Self::log_event("TCP_COMMAND", &format!("SENDING command '{}' to device {} - TCP_AVAILABLE: {}", command, device_id, tcp_available));
    }

    /// Logs successful TCP command transmission to ESP32 device.
    /// Records when command was successfully sent over TCP connection to device_id.
    pub fn log_tcp_command_success(device_id: &str, command: &str) {
        Self::log_event("TCP_COMMAND", &format!("SUCCESS sent command '{}' to device {}", command, device_id));
    }

    /// Logs failed TCP command transmission with error details.
    /// Records when command failed to send to device_id with error message for debugging connection issues.
    pub fn log_tcp_command_failed(device_id: &str, command: &str, error: &str) {
        Self::log_event("TCP_COMMAND", &format!("FAILED to send command '{}' to device {}: {}", command, device_id, error));
    }

    /// Logs TCP connection status changes for ESP32 device with status and details.
    /// Records connection state transitions (connected, disconnected, error) with descriptive details.
    pub fn log_tcp_connection_status(device_id: &str, status: &str, details: &str) {
        Self::log_event("TCP_CONNECTION", &format!("STATUS for device {}: {} - {}", device_id, status, details));
    }

    /// Logs TCP reconnection attempt for ESP32 device with reason for reconnect.
    /// Records when automatic reconnection is initiated and why (connection lost, timeout, error).
    pub fn log_tcp_reconnect_attempt(device_id: &str, reason: &str) {
        Self::log_event("TCP_RECONNECT", &format!("ATTEMPTING reconnect for device {} - reason: {}", device_id, reason));
    }

    /// Logs TCP reconnection result (success or failure) with optional error details.
    /// Records outcome of reconnection attempt, includes error message if reconnection failed.
    pub fn log_tcp_reconnect_result(device_id: &str, success: bool, error: Option<&str>) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        let details = if let Some(err) = error {
            format!(" - error: {}", err)
        } else {
            String::new()
        };
        Self::log_event("TCP_RECONNECT", &format!("RESULT for device {}: {}{}", device_id, status, details));
    }

    /// Clears all log files by deleting debug_events.log, tcp_messages.log, and templog.log.
    /// Removes all three log files to start fresh logging session. Ensures log directory exists before deletion.
    pub fn clear_log() {
        Self::ensure_log_dir();
        let _ = std::fs::remove_file(Self::LOG_FILE);
        let _ = std::fs::remove_file(Self::TCP_LOG_FILE);
        let _ = std::fs::remove_file(Self::TEMP_LOG_FILE);
    }

    /// Logs ESP32 device reset attempt with attempt number to temporary log.
    /// Records when reset command is initiated for device, tracks retry attempts with numbered sequence.
    pub fn log_reset_attempt(device_id: &str, attempt_number: u32) {
        Self::log_to_temp_log(&format!("RESET_ATTEMPT_{}: Device {} - Reset command initiated", attempt_number, device_id));
    }

    /// Logs successful ESP32 device reset with attempt number to temporary log.
    /// Records when reset command was sent successfully, correlates with attempt number for retry tracking.
    pub fn log_reset_success(device_id: &str, attempt_number: u32) {
        Self::log_to_temp_log(&format!("RESET_SUCCESS_{}: Device {} - Reset command sent successfully", attempt_number, device_id));
    }

    /// Logs failed ESP32 device reset with attempt number and error details to temporary log.
    /// Records reset failure with error message, tracks which retry attempt failed for debugging.
    pub fn log_reset_failure(device_id: &str, attempt_number: u32, error: &str) {
        Self::log_to_temp_log(&format!("RESET_FAILURE_{}: Device {} - Reset failed: {}", attempt_number, device_id, error));
    }

    /// Logs ESP32 connection drop event with reason to temporary log.
    /// Records when TCP/network connection to device is lost unexpectedly with reason for disconnection.
    pub fn log_connection_drop(device_id: &str, reason: &str) {
        Self::log_to_temp_log(&format!("CONNECTION_DROP: Device {} - Connection dropped: {}", device_id, reason));
    }

    /// Logs ESP32 device manager state changes to temporary log.
    /// Records internal state transitions of device manager for debugging device lifecycle and state machine.
    pub fn log_device_manager_state(device_id: &str, state: &str) {
        Self::log_to_temp_log(&format!("DEVICE_MANAGER_STATE: Device {} - {}", device_id, state));
    }

    /// Writes message to temporary log file (templog.log) with timestamp.
    /// Internal helper for logging device reset attempts, connection drops, and state changes to separate temporary log file.
    fn log_to_temp_log(message: &str) {
        Self::ensure_log_dir();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_entry = format!("[{}] {}\n", timestamp, message);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::TEMP_LOG_FILE) {
            let _ = file.write_all(log_entry.as_bytes());
            let _ = file.flush();
        }
    }
}