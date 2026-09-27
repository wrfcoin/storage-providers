# WRFCoin Storage Providers

[![Rust 1.70+](https://img.shields.io/badge/Rust-1.70%2B-000000?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Storage clients and tiering libraries for WRFCoin. The default storage-node testnet profile enables only a self-hosted Kubo/IPFS node. Filecoin and Arweave are not in this testnet. Incomplete paid-provider writes fail closed.

## 🏗 Architecture

The storage ecosystem is designed as a modular Rust workspace, ensuring type-safe and performant data handling across multiple decentralized protocols.

### Core Modules (`crates/`)

- **`storage-common`**: The foundation of the workspace. Defines the core `StorageProvider` and `ContentAddressed` traits, shared types (EnvironmentalReport, Metadata), error handling, and cryptographic integrity verification.
- **`ipfs`**: Hot/Warm storage provider. Handles rapid ingestion and retrieval of weather data blobs using Content Identifiers (CIDs).
- **`filecoin`**: Disabled (no working read or write path) / not in this testnet. Lotus authenticated writes are not implemented.
- **`arweave`**: Incomplete client — read-only / not in this testnet. Transaction signing is not implemented.
- **`data-tiering`**: The orchestrator. Implements intelligent migration of data between tiers based on age, frequency of access, and provenance policies.

## 📡 Data Tiering Model

| Tier | Provider | Access Latency | Primary Use Case |
|------|----------|----------------|------------------|
| **Hot** | Local Cache / Redis | < 100ms | Real-time weather observations |
| **Warm** | IPFS | < 5s | Active datasets and peer-to-peer sharing |
| **Cold** | Filecoin | Unavailable | Disabled (no working read or write path) / not in this testnet |
| **Archive** | Arweave | Unavailable | Client incomplete — read-only / not in this testnet |

## 🛡 Advanced Features

- **Provenance Chain Enforcement**: Every stored artifact is validated against a cryptographic provenance chain before being accepted into the archive.
- **Circuit Breaker & Failover**: The `FailoverRouter` in `storage-common` provides resilience against individual provider outages with automatic fallback logic.
- **Quarantine Flow**: Suspicious or malformed payloads (identified via `IntegrityVerifier`) are automatically routed to a quarantine metadata flow for manual or AI-assisted audit.
- **SLA Tracking**: Monitors availability and replication counts for "Public Interest" data classes to ensure minimum replica counts are maintained across the network.

## 🛠 Getting Started

### Prerequisites
- **Rust**: 1.70+ (Stable)
- **IPFS Node**: Required for local testing of the IPFS provider.
- **Credentials**: None for the testnet profile. Tests use loopback mocks.

### Installation
```bash
git clone https://github.com/wrfcoin/storage-providers.git
cd storage-providers
./scripts/fresh-clone-build.sh
```

### Development & Testing
```bash
# Check all crates in the workspace
cargo check --workspace

# Run tests for specific provider
cargo test -p wrfcoin-ipfs

# Run all storage integrity tests
cargo test --workspace
```

## Validation

Run `bash scripts/ci-local.selftest.sh` and `bash scripts/ci-local.sh` from the
workspace root. The gate checks formatting, warnings, workspace tests and the
fresh-clone build. Each step reports its exit status; success requires nonzero
passed tests. Exit 1 means failure; exit 2 means VOID (missing Cargo or zero tests).
The selftest exercises failing tests, missing Cargo, zero tests and a passing test.

The workflow targets the approved `[self-hosted, n5pro]` labels (core4#3232).
This repository currently has no CT102 runner: local invocation is the evidence
for the 2026-09-27 build contract. When a CT102 runner is assigned, run these same
commands there and record the head SHA, exit code and nonzero test count.
Queued, skipped or zero-step jobs are not validation evidence. The dependent
gate verifies that the build succeeded with executed steps; it cannot execute
until a runner is available. No paid provider or hosted runner is needed.

## Compatibility types

The repository vendors a compatibility copy of the environmental types from
`wrfcoin/meteo-types@b27a59fa8a1e8a7db589555ad697260f8a5ae36c`, so a fresh clone has no
sibling-checkout dependency. This is a neutral build mechanism, not a decision that standalone
`meteo-types` or Core4's fork is canonical; the oracle-lane decision will replace the compatibility
module when that ownership question is resolved.

---

Built with ❤️ for the global weather community.  
Part of the WRFCoin platform.

## IPFS-only testnet daemon

`STORAGE_NODE_PROFILE=testnet` is the default. It enables only `ipfs`; specifying
Filecoin or Arweave in `STORAGE_PROVIDERS` refuses startup. The testnet IPFS config
never reads API tokens; `IPFS_REMOTE_PINNING` is ignored with a warning, and remote
pinning clients are never constructed. `IPFS_IS_LOCAL=false` cannot enable paid
cost models in this profile. No wallet, remote gateway or paid pinning service is
required. Use a Kubo endpoint you operate.

`STORAGE_NODE_CONFIG=config/testnet.toml` loads the checked-in example (Kubo at
`http://ipfs:5001`, as a local compose service). For a native local Kubo, set
`IPFS_API_URL=http://127.0.0.1:5001`; this overrides the TOML IPFS endpoint.
The daemon requires successful Kubo uploads, and probes `/api/v0/id` with a
2-second timeout. `/health` returns 503 on probe failure. Kubo validates its
content-addressed blocks; the daemon preserves Kubo CIDs across reads rather
than comparing them to a different locally constructed identifier.

`/health` and `/stats` list all three provider postures. Filecoin/Arweave are
`not_in_testnet` with `healthy:null`; stats never invents probe results.
Store/retrieve requests selecting these providers return HTTP 501 with
`unavailable:true`, provider and reason. Cost endpoints include only active
providers and mark results `estimate_only:true`.

`STORAGE_NODE_PROFILE=full` is an experimental library-integration mode, not a
paid-provider readiness claim. Incomplete providers report `misconfigured` and
are excluded from cost estimates; missing analytics probes are unhealthy.

Build locally: `docker build -t storage-node:testnet .`. The multi-stage image
uses locked Cargo dependencies and a distroless non-root runtime. Building does
not deploy or push the image. Test the actual daemon without live services:
`cargo test -p wrfcoin-storage-node --test ipfs_only_profile` (seven loopback tests).
The repository has no CT102 runner; report these tests as local evidence.

## Deferred (needs funded wallets)

- Lotus Bearer authentication in `crates/filecoin/src/rpc_client.rs::rpc_call`.
  Setting the reserved `api_token` field does not enable writes in this build.
  Filecoin provider store/delete and administrative `Filecoin.Client*` RPC calls
  return `NotConfigured` without network traffic.
- Arweave JWK RSA-PSS signing in a future `crates/arweave/src/signer.rs`.
  Provider store and gateway submit_transaction return `NotConfigured`;
  gateway get_data, get_transaction and get_price remain available.

The storage-node API maps these errors to HTTP 501. No wallet or token is needed
for local regression tests; HTTP tests use loopback mocks only.
