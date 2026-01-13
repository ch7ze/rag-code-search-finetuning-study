use axum::{
    body::Body,
    http::StatusCode,
    response::Response,
};
use std::{fs, time::UNIX_EPOCH};


// ============================================================================
// SPA ROUTE HANDLER - Serviert die Haupt-HTML Datei für alle SPA-Routes
// Website-Feature: Alle URLs (/, /login, /register, etc.) zeigen die gleiche HTML
// ============================================================================

// Serviert eine spezifische Template-Datei
pub async fn handle_template_file(file_path: &str, cache_control: &str) -> Response<Body> {
    
    // Versuche die Template-Datei zu lesen
    match fs::read_to_string(file_path) {
        Ok(contents) => {
            // ETag für Client-seitiges Caching erstellen
            // ETag = "Entity Tag" - eindeutige Kennung für Datei-Version
            let etag = match fs::metadata(file_path) {
                Ok(metadata) => {
                    let size = metadata.len();  // Dateigröße
                    // Letzte Änderungszeit als Unix-Timestamp
                    let modified = metadata
                        .modified()
                        .unwrap_or(UNIX_EPOCH)
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    // ETag = "größe-zeitstempel"
                    format!("\"{}-{}\"", size, modified)
                }
                Err(_) => "\"default-etag\"".to_string(),
            };
            
            // HTTP Response erstellen
            Response::builder()
                .header("content-type", "text/html; charset=utf-8") // HTML mit UTF-8
                .header("etag", etag)                              // Caching-Header
                .header("cache-control", cache_control)            // Konfigurierbares Caching
                .body(Body::from(contents))                         // HTML Content
                .unwrap()
        }
        Err(err) => {
            // Fehler-Handling wenn Datei nicht gelesen werden kann
            eprintln!("Error reading {}: {}", file_path, err);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)  // HTTP 500
                .body(Body::from("Error loading content"))
                .unwrap()
        }
    }
}


// ============================================================================
// RUST KONZEPTE IN DIESER DATEI:
// 
// 1. Error Handling mit Result<T, E> und ? Operator
// 2. Pattern Matching mit match expressions
// 3. Option<T> mit .and_then() und .unwrap_or()
// 4. String vs &str (owned vs borrowed strings)
// 5. File I/O mit std::fs
// 6. HTTP Response Building mit Builder Pattern
// 7. Static Lifetimes mit &'static str
// ============================================================================