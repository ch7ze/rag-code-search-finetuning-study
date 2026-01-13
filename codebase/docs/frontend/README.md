# Frontend Architecture Documentation

## Überblick

Das Frontend ist eine moderne Single Page Application (SPA), die eine interaktive Canvas-basierte Zeichenanwendung mit Multi-User-Kollaboration bereitstellt. Die Architektur kombiniert TypeScript für typisierte Canvas-Operationen mit Vanilla JavaScript für die SPA-Funktionalität.

## Technologie-Stack

- **SPA Framework**: Vanilla JavaScript Router
- **Canvas Engine**: TypeScript + HTML5 Canvas
- **Build System**: Node.js + TypeScript Compiler
- **WebSocket**: Native WebSocket für Echtzeit-Kommunikation
- **Styling**: CSS3 mit Component-basiertem Ansatz
- **Module System**: ES6 Modules

## Projektstruktur

```
client/
├── app.js                 # SPA Router und Navigation
├── index.html            # Haupt-Shell der Application
├── index.css             # Globale Styles
├── scripts/
│   ├── websocket-client.js      # WebSocket Kommunikation
│   ├── canvas-websocket-bridge.js  # Canvas-WebSocket Integration
│   ├── event-system.js          # Event Management System
│   ├── color-state.js           # Farb-State Management
│   ├── drawer-state.js          # Drawer State Management
│   ├── drawer_page.js           # Drawer Page Controller
│   ├── drawing_board.js         # Drawing Board Controller
│   ├── menu-api.js              # Menu API Integration
│   └── drawer/                  # TypeScript Canvas Engine
│       ├── index.ts             # Main Entry Point
│       ├── models.ts            # Data Models & Types
│       ├── canvas.ts            # Canvas Management
│       ├── abstract-shapes.ts   # Shape Base Classes
│       ├── shape-factory.ts     # Shape Creation Factory
│       ├── tools.ts             # Drawing Tools
│       ├── events.ts            # Canvas Event System
│       ├── event-bus.ts         # Event Bus Implementation
│       ├── event-store.ts       # Local Event Storage
│       ├── event-wrapper.ts     # Event Wrapper Utilities
│       ├── utils.ts             # Utility Functions
│       ├── constants.ts         # Application Constants
│       └── shapes/              # Shape Implementations
│           ├── line.ts          # Line Shape
│           ├── circle.ts        # Circle Shape
│           ├── rectangle.ts     # Rectangle Shape
│           └── triangle.ts      # Triangle Shape
├── styles/
│   ├── drawer_page.css          # Drawer Page Styles
│   ├── drawing_board.css        # Drawing Board Styles
│   └── menu-api.css             # Menu API Styles
└── templates/
    ├── index.html               # Home Template
    ├── login.html               # Login Template
    ├── register.html            # Registration Template
    ├── drawer_page.html         # Drawer Template
    ├── drawing_board.html       # Drawing Board Template
    ├── canvas_detail.html       # Canvas Detail Template
    ├── about.html               # About Template
    ├── admin.html               # Admin Template
    ├── debug.html               # Debug Template
    └── hallo.html               # Hello Template
```

## Architektur-Komponenten

### 1. SPA Router System (app.js)

#### Funktionalitäten
- **Client-Side Routing**: Navigation ohne Page Refresh
- **Template Loading**: Dynamisches Laden von HTML-Templates
- **Script Management**: Automatisches Laden von Page-spezifischen Scripts
- **Authentication**: Route-basierte Authentifizierungsprüfung

#### Page Configuration
```javascript
const pages = {
    'drawer_page': {
        title: 'Drawer',
        template: 'drawer_page.html',
        scripts: ['websocket-client.js', 'drawer_page.js'],
        styles: ['drawer_page.css'],
        requiresAuth: true
    }
};
```

#### Route Management
- **Static Routes**: Vordefinierte Seiten
- **Dynamic Routes**: Canvas-spezifische Routen (`/canvas/:id`)
- **Fallback Handling**: 404-Behandlung
- **Authentication Guards**: Automatische Login-Umleitung

### 2. Canvas Engine (drawer/ TypeScript Modules)

#### Core Components

##### Models & Types (models.ts)
- **Point2D**: 2D-Koordinaten-System
- **Shape Interface**: Abstrakte Shape-Definition
- **ShapeManager**: Canvas Shape Management
- **DrawingContext**: Canvas-Kontext Abstraktion

##### Canvas Management (canvas.ts)
- **Canvas Initialization**: Setup und Konfiguration
- **Rendering Engine**: Shape-Rendering auf Canvas
- **Event Handling**: Mouse/Touch-Events
- **Coordinate Transformation**: Screen-zu-Canvas Koordinaten

##### Shape System (abstract-shapes.ts + shapes/)
- **AbstractShape**: Basis-Klasse für alle Shapes
- **Shape Factories**: Factory Pattern für Shape-Erstellung
- **Polymorphic Rendering**: Einheitliches Render-Interface
- **Shape Serialization**: JSON-Export/Import von Shapes

#### Drawing Tools (tools.ts)
- **SelectionTool**: Shape-Auswahl und -Manipulation
- **DrawingTools**: Line, Circle, Rectangle, Triangle Tools
- **Tool State Management**: Aktives Tool Tracking
- **Tool Area Management**: UI-Integration

### 3. Event System

#### Event Bus (event-bus.ts)
- **Publish/Subscribe Pattern**: Entkoppelte Kommunikation
- **Type-Safe Events**: TypeScript Event-Typen
- **Event Filtering**: Selektive Event-Subscription
- **Error Handling**: Robuste Event-Verarbeitung

#### Event Store (event-store.ts)
- **Local Event History**: Client-seitige Event-Speicherung
- **Event Replay**: Wiederherstellung von Canvas-Zustand
- **Undo/Redo Support**: Event-basierte Rückgängig-Funktion
- **Event Compression**: Optimierung der Event-Historie

#### Event Wrapper (event-wrapper.ts)
- **Event Normalization**: Einheitliche Event-Struktur
- **Metadata Enrichment**: Zusätzliche Event-Informationen
- **Validation**: Event-Validierung vor Verarbeitung
- **Serialization**: Event-Serialisierung für WebSocket

### 4. WebSocket Integration

#### WebSocket Client (websocket-client.js)
- **Connection Management**: Automatische Reconnection
- **Message Handling**: Typed Message Processing
- **Connection State**: Online/Offline Status
- **Error Recovery**: Robuste Fehlerbehandlung

#### Canvas-WebSocket Bridge (canvas-websocket-bridge.js)
- **Event Synchronization**: Canvas Events -> WebSocket Messages
- **Remote Event Processing**: WebSocket Messages -> Canvas Events
- **Conflict Resolution**: Multi-User Event Konflikte
- **Performance Optimization**: Event Batching

### 5. State Management

#### Color State (color-state.js)
- **Color Picker Integration**: UI-Farb-Auswahl
- **Color History**: Zuletzt verwendete Farben
- **Palette Management**: Vordefinierte Farbpaletten
- **Color Validation**: Gültige Farb-Formate

#### Drawer State (drawer-state.js)
- **Canvas State**: Aktueller Canvas-Zustand
- **Tool State**: Aktives Zeichenwerkzeug
- **Selection State**: Ausgewählte Shapes
- **Mode State**: Zeichnen vs. Auswählen

### 6. Page Controllers

#### Drawer Page (drawer_page.js)
- **Canvas Initialization**: Drawer Engine Setup
- **UI Event Binding**: Button-Clicks, Tool-Auswahl
- **State Synchronization**: UI <-> Canvas State
- **WebSocket Integration**: Real-time Collaboration

#### Drawing Board (drawing_board.js)
- **Simple Drawing**: Basis-Zeichenfunktionalität
- **Tool Management**: Einfache Tool-Auswahl
- **Canvas Export**: Drawing -> Image Export
- **Local Storage**: Lokale Drawing-Persistierung

#### Menu API (menu-api.js)
- **Canvas Management**: Canvas CRUD-Operationen
- **User Management**: Benutzer-Verwaltung
- **Permission Management**: Canvas-Berechtigungen
- **API Communication**: REST API Integration

## Build System

### TypeScript Compilation
```json
{
  "compilerOptions": {
    "module": "es6",
    "target": "es6",
    "sourceMap": true,
    "outDir": "./client/scripts",
    "rootDir": "./src"
  }
}
```

### Build Process
1. **TypeScript -> JavaScript**: Kompilierung mit Source Maps
2. **Asset Copying**: Templates, Styles -> dest/
3. **Cache Busting**: Hash-basierte Asset-Versionierung
4. **Minification**: CSS/JS Minimierung für Production

### Development Workflow
```bash
# Development Mode
npm run dev              # TypeScript Watch + Rust Watch
npm run dev-frontend     # Nur TypeScript Watch
npm run dev-backend      # Nur Rust Watch

# Production Build
npm run build           # Full Production Build
npm start              # Build + Server Start
```

## Routing System

### Route Definitions
- **Static Routes**: `/login`, `/register`, `/about`
- **Authenticated Routes**: `/drawer_page`, `/drawing_board`
- **Dynamic Routes**: `/canvas/:canvas_id`
- **API Routes**: `/api/*` (Proxy to Backend)

### Navigation Flow
1. **URL Change**: Browser History API
2. **Route Matching**: Pattern-basierte Route-Auflösung
3. **Authentication Check**: Login-Status Validierung
4. **Template Loading**: Asynchrones Template-Laden
5. **Script Loading**: Page-spezifische Script-Injection
6. **Component Initialization**: Page Controller Setup

## WebSocket Protocol

### Message Types
```typescript
interface CanvasEvent {
    type: 'shape_created' | 'shape_modified' | 'shape_deleted' | 'cursor_moved';
    canvas_id: string;
    user_id: string;
    data: any;
    timestamp: string;
}
```

### Event Flow
1. **Local Action**: Benutzer-Interaktion auf Canvas
2. **Event Creation**: TypeScript Event-Objekt erstellen
3. **Local Processing**: Sofortige lokale Canvas-Aktualisierung
4. **WebSocket Send**: Event an Server senden
5. **Server Broadcast**: Event an alle Canvas-Teilnehmer
6. **Remote Processing**: Event von anderen Clients verarbeiten

## Styling Architecture

### CSS Organization
- **Global Styles**: `index.css` für Application-weite Styles
- **Component Styles**: Page-spezifische CSS-Dateien
- **Utility Classes**: Wiederverwendbare CSS-Klassen
- **Responsive Design**: Mobile-First Approach

### Style Loading
- **Dynamic Loading**: CSS wird per Page geladen
- **Cache Management**: Browser-Caching für Performance
- **Hot Reload**: Development-Zeit CSS-Updates

## Performance Optimierungen

### Template Caching
- **In-Memory Cache**: Geladene Templates im Speicher
- **Lazy Loading**: Templates nur bei Bedarf laden
- **Preloading**: Kritische Templates vorläufig laden

### Canvas Optimierungen
- **Basic Rendering**: Canvas-Rendering mit HTML5 Canvas API

### Network Optimierungen
- **WebSocket Keepalive**: Verbindung offen halten
- **Reconnection Strategy**: Intelligente Wiederverbindung

## Security Considerations

### Client-Side Security
- **Input Validation**: Alle User-Inputs validieren
- **Secure WebSocket**: WebSocket-Verbindungen über HTTP

### Authentication Integration
- **JWT Token Storage**: HTTP-Only Cookies
- **Automatic Login**: Session-basierte Authentifizierung
- **Permission Checks**: Canvas-Berechtigungen im Frontend

## Testing Strategy

### Unit Testing
```javascript
// Canvas Engine Tests
describe('ShapeFactory', () => {
    it('should create valid line shape', () => {
        const line = LineFactory.create(point1, point2);
        expect(line.type).toBe('line');
    });
});
```

### Integration Testing
- **WebSocket Testing**: Mock WebSocket Server
- **Canvas Testing**: Headless Canvas Simulation
- **API Integration**: Mock Backend Responses
- **E2E Testing**: Full User Workflow Tests

### Test Organization
```
tests/frontend/
├── unit/
│   ├── canvas.test.js
│   ├── shapes.test.js
│   └── events.test.js
└── e2e/
    └── drawing.spec.js
```

## Development Guidelines

### Code Organization
- **Module Pattern**: ES6 Modules für alle Komponenten
- **Single Responsibility**: Jede Datei hat einen klaren Zweck
- **Dependency Injection**: Lose gekoppelte Komponenten
- **Interface Segregation**: Kleine, fokussierte Interfaces

### TypeScript Best Practices
- **Strict Mode**: Vollständige Type-Safety
- **Interface-basiert**: Klare Vertrags-Definitionen
- **Null Safety**: Explicit null/undefined Handling
- **Generic Types**: Wiederverwendbare Type-Definitions