# WRFCoin Storage Providers

[![Rust 1.70+](https://img.shields.io/badge/Rust-1.70%2B-000000?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

**Decentralized Storage Ecosystem** for the WRFCoin platform. This repository provides high-performance Rust integrations for IPFS, Filecoin, and Arweave, managed by a sophisticated hot/warm/cold data tiering orchestrator.

## 🏗 Architecture

The storage ecosystem is designed as a modular Rust workspace, ensuring type-safe and performant data handling across multiple decentralized protocols.

### Core Modules (`crates/`)

- **`storage-common`**: The foundation of the workspace. Defines the core `StorageProvider` and `ContentAddressed` traits, shared types (EnvironmentalReport, Metadata), error handling, and cryptographic integrity verification.
- **`ipfs`**: Hot/Warm storage provider. Handles rapid ingestion and retrieval of weather data blobs using Content Identifiers (CIDs).
- **`filecoin`**: Cold storage provider. Manages long-term archival of historical datasets through automated storage deal negotiation.
- **`arweave`**: Permanent storage provider. Ensures climate records and critical environmental evidence are stored forever on a permaweb layer.
- **`data-tiering`**: The orchestrator. Implements intelligent migration of data between tiers based on age, frequency of access, and provenance policies.

## 📡 Data Tiering Model

| Tier | Provider | Access Latency | Primary Use Case |
|------|----------|----------------|------------------|
| **Hot** | Local Cache / Redis | < 100ms | Real-time weather observations |
| **Warm** | IPFS | < 5s | Active datasets and peer-to-peer sharing |
| **Cold** | Filecoin | Minutes | Historical archival and bulk data backup |
| **Archive** | Arweave | Minutes | Permanent, immutable climate benchmarks |

## 🛡 Advanced Features

- **Provenance Chain Enforcement**: Every stored artifact is validated against a cryptographic provenance chain before being accepted into the archive.
- **Circuit Breaker & Failover**: The `FailoverRouter` in `storage-common` provides resilience against individual provider outages with automatic fallback logic.
- **Quarantine Flow**: Suspicious or malformed payloads (identified via `IntegrityVerifier`) are automatically routed to a quarantine metadata flow for manual or AI-assisted audit.
- **SLA Tracking**: Monitors availability and replication counts for "Public Interest" data classes to ensure minimum replica counts are maintained across the network.

## 🛠 Getting Started

### Prerequisites
- **Rust**: 1.70+ (Stable)
- **IPFS Node**: Required for local testing of the IPFS provider.
- **Arweave/Filecoin API Keys**: Required for integration testing.

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
