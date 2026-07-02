FROM rust:1.96.1-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin hyper-example

FROM debian:trixie-20260623-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hyper-example /app/
EXPOSE 8080
CMD ["/app/hyper-example"]
