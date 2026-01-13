# Projekt Übersicht - Drawing App

## Architekturelle Abweichungen und Begründungen

### 1. SQLite statt In-Memory Storage
**Abweichung**: Verwendung von SQLite-Datenbank statt In-Memory Storage  
**Begründung**: 
- Persistenz der Benutzer- und Canvas-Daten über Server-Neustarts hinweg
- Realistische Production-Umgebung
- Bessere Testbarkeit durch konsistente Datengrundlage
- Ermöglicht erweiterte Features wie User-Suche und Canvas-Management

### 2. Erweiterte Permission-APIs
**Abweichung**: Umfassende REST-APIs für Canvas- und Permission-Management  
**Begründung**:
- Benutzerfreundliche Verwaltung von Canvas-Berechtigungen
- Separation of Concerns zwischen Authentication und Authorization
- Skalierbare Architektur für größere Nutzermengen
- Professionelle Web-Anwendungs-Standards



## Technische Highlights

### Backend (Rust)
- **Framework**: Axum mit Tokio Async Runtime
- **Datenbank**: SQLite mit SQLx für type-safe Queries
- **Authentication**: JWT mit bcrypt Password-Hashing
- **WebSocket**: Tokio-Tungstenite für Real-time Communication
- **Architecture**: Modularer Ansatz mit klarer Trennung der Verantwortlichkeiten

### Frontend (TypeScript/JavaScript)
- **Architecture**: Single Page Application mit Custom Router
- **Canvas Engine**: TypeScript-basierte Canvas-Implementierung
- **Real-time**: WebSocket Client mit automatischer Reconnection
- **State Management**: Event-Bus Pattern für lose Kopplung
- **Build System**: TypeScript-to-JavaScript Compilation

### Datenbank-Design
```sql
-- User Management
users (id, email, display_name, password_hash, created_at, is_admin)

-- Canvas Management  
canvas (id, name, owner_id, is_moderated, created_at)

-- Permission System
canvas_permissions (canvas_id, user_id, permission, granted_at)
```

### Konfiguration
- **Initiale Benutzer**: `data/initial_users.json`
- **Database**: `data/users.db` (wird automatisch erstellt)
- **Static Assets**: Cache-Busting über `client-hash.json`

## Testabdeckung



### Backend Tests
- **Unit Tests**: Alle Module haben isolierte Tests
- **Integration Tests**: API-Endpoint Testing
- **Database Tests**: SQLite-Integration Testing

### Frontend Tests  
- **Unit Tests**: Canvas Engine und Komponenten
- **E2E Tests**: Multi-User Collaboration Workflows
- **WebSocket Tests**: Real-time Communication

## Security Features

### Implemented Security Measures
- **JWT Security**: HTTP-Only Cookies mit sicherem Signierung
- **Password Security**: bcrypt mit Salt-Rounds
- **SQL Injection Prevention**: SQLx Prepared Statements  
- **Input Validation**: Serde-basierte Request/Response Validation
- **Permission Checks**: Server-side Authorization für alle Operationen

## Qualitätssicherung

### Code Quality
- **Rust**: Clippy Linting, Cargo Format
- **TypeScript**: Strict Mode, ESLint
- **Documentation**: Inline Code Documentation
- **Version Control**: Strukturierte Git History

### Performance
- **Async Operations**: Non-blocking I/O durchgehend
- **Connection Pooling**: SQLite Connection Management
- **Caching**: Hash-basierte Asset-Caching-Strategien
- **WebSocket Optimization**: Connection Cleanup und Selective Broadcasting

