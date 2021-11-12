FROM lukemathwalker/cargo-chef:latest-rust-1.56-slim-buster AS chef
WORKDIR /app

# Compute a lock-like file for our build
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build project deps, not the app
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin pid-tester

# Build the runtime image
FROM ubuntu:20.04 AS runtime
WORKDIR /app
RUN apt-get update && \
    apt-get upgrade -y && \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/pod-generator ./pod-generator
ENTRYPOINT [ "./pod-generator" ]