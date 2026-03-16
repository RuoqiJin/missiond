FROM rust:1.84-bookworm AS builder

WORKDIR /app

# Copy workspace root + all member Cargo.toml for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/skill-store/Cargo.toml crates/skill-store/Cargo.toml
COPY crates/missiond-core/Cargo.toml crates/missiond-core/Cargo.toml
COPY crates/missiond-runner/Cargo.toml crates/missiond-runner/Cargo.toml
COPY crates/missiond-mcp/Cargo.toml crates/missiond-mcp/Cargo.toml
COPY crates/missiond-daemon/Cargo.toml crates/missiond-daemon/Cargo.toml
COPY crates/missiond-attach/Cargo.toml crates/missiond-attach/Cargo.toml
COPY crates/semantic-terminal-napi/Cargo.toml crates/semantic-terminal-napi/Cargo.toml

# Create dummy mains for all workspace members
RUN for d in skill-store missiond-core missiond-runner missiond-mcp missiond-daemon missiond-attach; do \
      mkdir -p crates/$d/src && echo "fn main() {}" > crates/$d/src/main.rs && echo "" > crates/$d/src/lib.rs; \
    done && \
    mkdir -p crates/semantic-terminal-napi/src && echo "" > crates/semantic-terminal-napi/src/lib.rs

# Pre-build dependencies (cached layer)
RUN cargo build --release -p skill-store 2>/dev/null || true

# Copy actual skill-store source only
COPY crates/skill-store/src crates/skill-store/src
RUN touch crates/skill-store/src/main.rs && cargo build --release -p skill-store

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/skill-store /usr/local/bin/skill-store

EXPOSE 8900
ENV RUST_LOG=skill_store=info
CMD ["skill-store"]
