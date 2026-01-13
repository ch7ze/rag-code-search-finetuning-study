# Backend Architecture Documentation

## Überblick

Das Backend ist eine moderne Rust-Webanwendung basierend auf dem Axum-Framework, die als Multi-User Canvas Drawing Application dient. Die Architektur folgt einem modularen Ansatz mit klarer Trennung der Verantwortlichkeiten.

## Technologie-Stack

- **Framework**: Axum (Rust Web Framework)
- **Runtime**: Tokio (Async Runtime)
- **Datenbank**: SQLite mit SQLx
- **WebSocket**: Tokio-Tungstenite
- **Authentifizierung**: JWT mit bcrypt
- **Logging**: Tracing

## Projektstruktur

```
src/backend/
├── main.rs           # Entry Point und HTTP-Router
├── lib.rs           # Library Export für Tests
├── auth.rs          # Authentifizierung und JWT-Management
├── database.rs      # SQLite Datenbankschicht
├── events.rs        # Canvas Event Definitionen
├── canvas_store.rs  # In-Memory Event Store
├── websocket.rs     # WebSocket Handler für Multiuser
└── file_utils.rs    # File Serving und SPA Routing
```

## Architektur-Komponenten

### 1. HTTP Server (main.rs)
- **Entry Point**: Startet den Axum Web Server auf Port 3000
- **Router Configuration**: Definiert alle API-Endpunkte
- **State Management**: Verwaltet globalen Application State
- **Middleware**: Logging und Request Tracing

### 2. Authentifizierung (auth.rs)
- **JWT-basierte Authentifizierung**: Sichere Token-Verwaltung
- **Passwort Hashing**: bcrypt für sichere Passwort-Speicherung
- **Cookie-Management**: HTTP-Only Cookies für Sicherheit
- **User Management**: Registration, Login, Profile Updates

### 3. Datenbank Layer (database.rs)
- **SQLite Integration**: Leichtgewichtige eingebettete Datenbank
- **Async Operations**: Vollständig asynchrone Datenbankoperationen
- **Migration Support**: Automatische Schema-Initialisierung
- **Canvas Permissions**: Benutzerberechtigungen für Canvas-Management

### 4. WebSocket System (websocket.rs)
- **Multiuser Support**: Echtzeit-Kollaboration zwischen Nutzern
- **Event Broadcasting**: Verteilung von Canvas-Events an alle Clients
- **Connection Management**: Automatische Cleanup von toten Verbindungen
- **Statistics Tracking**: Monitoring von aktiven WebSocket-Verbindungen

### 5. Event Store (canvas_store.rs)
- **In-Memory Event Storage**: Schneller Zugriff auf Canvas-Events
- **Thread-Safe Operations**: Arc/RwLock für sichere Concurrent-Access
- **Event Streaming**: Replay von Canvas-Events für neue Clients
- **Memory Management**: Automatische Bereinigung alter Events

### 6. File Serving (file_utils.rs)
- **SPA Support**: Single Page Application Routing
- **Static Assets**: Serving von CSS, JS und HTML-Dateien
- **Cache Management**: Hash-basierte Cache-Busting-Strategie
- **Template Handling**: Dynamisches Serving von HTML-Templates

## API-Endpunkte

### Authentifizierung
- `POST /api/register` - Benutzer-Registrierung
- `POST /api/login` - Benutzer-Anmeldung
- `POST /api/logout` - Benutzer-Abmeldung
- `GET /api/validate-token` - Token-Validierung
- `GET /api/user-info` - Benutzer-Informationen
- `PUT /api/profile/display-name` - Anzeigename ändern

### Canvas Management
- `GET /api/canvas` - Liste aller Canvas des Benutzers
- `POST /api/canvas` - Neue Canvas erstellen
- `GET /api/canvas/:id` - Canvas-Details
- `PUT /api/canvas/:id` - Canvas-Eigenschaften ändern
- `DELETE /api/canvas/:id` - Canvas löschen
- `POST /api/canvas-permissions/:id` - Berechtigungen verwalten

### User Management
- `GET /api/users/search` - Benutzer-Suche
- `GET /api/users/list` - Benutzer-Liste

### WebSocket & Monitoring
- `GET /channel` - WebSocket-Verbindung für Canvas-Events
- `GET /api/websocket/stats` - WebSocket-Statistiken
- `GET /api/canvas/:canvas_id/users` - Aktive Canvas-Nutzer

## Datenbank Schema

### Tabelle: users
- `id` (TEXT PRIMARY KEY) - Eindeutige Benutzer-ID
- `email` (TEXT UNIQUE) - E-Mail-Adresse
- `display_name` (TEXT) - Anzeigename
- `password_hash` (TEXT) - Gehashtes Passwort
- `created_at` (TIMESTAMP) - Erstellungszeitpunkt

### Tabelle: canvas
- `id` (TEXT PRIMARY KEY) - Canvas-ID
- `name` (TEXT) - Canvas-Name
- `owner_id` (TEXT) - Besitzer-ID
- `is_moderated` (BOOLEAN) - Moderationsstatus
- `created_at` (TIMESTAMP) - Erstellungszeitpunkt

### Tabelle: canvas_permissions
- `canvas_id` (TEXT) - Canvas-Referenz
- `user_id` (TEXT) - Benutzer-Referenz
- `permission` (TEXT) - Berechtigung (R/W/V/M/O)

## Berechtigungssystem

### Permission Levels
- **R** (Read) - Canvas anzeigen
- **W** (Write) - Zeichnen auf Canvas
- **V** (Validate) - Moderations-Entscheidungen treffen
- **M** (Moderate) - Benutzer-Berechtigungen verwalten
- **O** (Owner) - Vollzugriff auf Canvas

### Permission Hierarchie
- Owner (O) kann alle Berechtigungen vergeben
- Moderator (M) kann R, W, V Berechtigungen vergeben
- Validate (V) kann Moderations-Entscheidungen treffen
- Write (W) kann zeichnen
- Read (R) kann nur anzeigen

## WebSocket Event System

### Event Types
- `ShapeCreated` - Neue Form gezeichnet
- `ShapeModified` - Form verändert
- `ShapeDeleted` - Form gelöscht
- `CursorMoved` - Cursor-Position aktualisiert
- `UserJoined` - Benutzer der Canvas beigetreten
- `UserLeft` - Benutzer die Canvas verlassen

### Event Flow
1. Client sendet Event über WebSocket
2. Server validiert Berechtigung
3. Event wird im Canvas Store gespeichert
4. Event wird an alle verbundenen Clients gebroadcast
5. Clients aktualisieren ihre lokale Canvas-Darstellung

## Sicherheit

### Authentifizierung
- JWT-Tokens mit HMAC-SHA256 Signierung
- HTTP-Only Cookies gegen XSS
- Sichere Passwort-Hashing mit bcrypt

### Autorisierung
- Granulare Canvas-Berechtigungen
- JWT-Claims mit Canvas-Permissions
- Request-Level Permission-Checks

### Input Validation
- Strukturierte Request/Response mit Serde
- Längen-Validierung für User-Inputs
- SQL Injection Prevention durch SQLx

## Performance Optimierungen

### In-Memory Caching
- Canvas Events im RAM für schnellen Zugriff
- User Sessions im Memory Store
- Hash-basierte Static Asset-Caching

### Asynchrone Verarbeitung
- Tokio Runtime für Non-Blocking I/O
- Concurrent Request Handling
- Async Database Operations

### WebSocket Optimierungen
- Connection Pooling
- Selective Event Broadcasting
- Automatic Dead Connection Cleanup

## Deployment Considerations

### Container Support
- Dockerfile für containerized Deployment
- Volume-Mapping für SQLite-Datei

### Configuration
- Database Path Configuration

### Monitoring
- Structured Logging mit Tracing
- WebSocket Statistics API

## Development Setup

### Prerequisites
```bash
# Rust Installation
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update

# Dependencies
cargo install cargo-watch
```

### Build & Run
```bash
# Development Mode
cargo run

# Production Build
cargo build --release

# Tests
cargo test

# Watch Mode
cargo watch -x run
```

### Database Setup
Die SQLite-Datenbank wird automatisch beim ersten Start erstellt:
- Location: `data/users.db`
- Initial Users: `data/initial_users.json`

## Testing Strategy

### Unit Tests
- Alle Module haben dedicated Unit Tests
- Mock Database für isolierte Tests
- Async Test Support mit tokio-test

### Integration Tests
- API Endpoint Testing
- Database Integration Tests
- WebSocket Connection Tests

### Test Structure
```
tests/
├── backend_unit_tests.rs
├── backend_integration_tests.rs
└── backend/
    ├── unit/
    └── integration/
```