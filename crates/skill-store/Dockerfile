FROM rust:1.84-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/skill-store/Cargo.toml crates/skill-store/Cargo.toml
# Create dummy src for dependency caching
RUN mkdir -p crates/skill-store/src && echo "fn main() {}" > crates/skill-store/src/main.rs
RUN cargo build --release -p skill-store 2>/dev/null || true

# Copy actual source
COPY crates/skill-store/src crates/skill-store/src
RUN touch crates/skill-store/src/main.rs && cargo build --release -p skill-store

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/skill-store /usr/local/bin/skill-store

EXPOSE 8900
ENV RUST_LOG=skill_store=info
CMD ["skill-store"]
