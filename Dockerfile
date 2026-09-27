FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p wrfcoin-storage-node
RUN mkdir /runtime-libs && cp /usr/lib/$(gcc -dumpmachine)/libssl.so.3 /usr/lib/$(gcc -dumpmachine)/libcrypto.so.3 /runtime-libs/

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /runtime-libs/ /usr/lib/
COPY --from=builder /src/target/release/storage-node /app/storage-node
COPY config/testnet.toml /app/config/testnet.toml
ENV STORAGE_NODE_PROFILE=testnet STORAGE_NODE_CONFIG=/app/config/testnet.toml
USER nonroot:nonroot
EXPOSE 7001
ENTRYPOINT ["/app/storage-node"]
