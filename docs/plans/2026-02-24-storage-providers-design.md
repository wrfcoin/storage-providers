# Storage Providers Design

## Context

Issue #371 describes decentralized storage integration for WRFCoin weather data. Reference code
exists in `blockchain-core/storage/` (~22K lines) with meaningful IPFS and Arweave implementations,
data tiering, and migration frameworks. No Filecoin provider existed.

## Architecture

Rust-only workspace with 5 crates: shared traits, three storage providers, and a data tiering
orchestrator. Storage providers run as sidecar services communicating with native-chain via HTTP/gRPC.

## Core Trait

```rust
#[async_trait]
pub trait StorageProvider: Send + Sync {
    async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId>;
    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>>;
    async fn exists(&self, id: &ContentId) -> Result<bool>;
    async fn delete(&self, id: &ContentId) -> Result<()>;
    async fn health_check(&self) -> Result<HealthStatus>;
    async fn stats(&self) -> Result<StorageStats>;
}
```

## Integration with core4

native-chain transaction payloads reference CIDs. The storage provider resolves them. The
`storage-common` crate defines the trait that `core4/shared-protocol` can depend on.

## Decisions

- **Language**: Rust-only (storage providers are backend sidecar services, not EVM-facing)
- **Scope**: All three providers + data tiering orchestrator
- **Source**: Fresh implementations using blockchain-core/storage/ as reference
- **Priority**: IPFS first (most mature, best fit for weather data blobs)
