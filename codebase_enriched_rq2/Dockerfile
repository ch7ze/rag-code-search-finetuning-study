# Multi-stage build: Rust + Node.js
FROM node:18 as base

# Rust installieren
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# SQLite und Build-Tools installieren
RUN apt-get update && apt-get install -y \
    sqlite3 \
    libsqlite3-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Arbeitsverzeichnis festlegen
WORKDIR /app

# Package-Dateien kopieren
COPY package*.json ./
COPY Cargo.toml Cargo.lock ./

# Node.js Abhängigkeiten installieren
RUN npm install

# Rust Dependencies cachen
RUN mkdir -p src/backend && echo "fn main() {}" > src/backend/main.rs
RUN cargo build --release || true
RUN rm -rf src/

# Alle Dateien kopieren
COPY . .

# TypeScript kompilieren und Assets bundeln
RUN npm run build

# Rust Backend kompilieren
RUN cargo build --release

# Port freigeben
EXPOSE 3000

# Environment variables setzen (wie in launch.json)
ENV RUST_LOG="info,drawing_app_backend=debug"

# Startbefehl (wie 🚀 Run Full App (Production))
CMD ["npm", "start"]