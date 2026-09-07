FROM rust:1.98.0-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin hyper-example

FROM debian:trixie-20260824-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hyper-example /app/
EXPOSE 8080
CMD ["/app/hyper-example"]
