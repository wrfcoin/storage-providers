# WRFCoin Storage Providers

Decentralized storage integrations for WRFCoin weather data. IPFS, Filecoin, and Arweave providers
with a hot/warm/cold data tiering orchestrator.

Related: [issue #371](https://github.com/wrfcoin/core4/issues/371)

## Structure

```
crates/
├── storage-common/   # Shared StorageProvider trait, types, errors
├── ipfs/             # IPFS provider (weather data blobs, CID references)
├── filecoin/         # Filecoin provider (archival with storage deals)
├── arweave/          # Arweave provider (permanent climate records)
└── data-tiering/     # Hot/warm/cold orchestrator with migration
```

## Data Tiering Model

| Tier | Provider | Access Time | Use Case |
|------|----------|-------------|----------|
| Hot | Local/Redis | Sub-second | Recent weather observations |
| Warm | IPFS | Seconds | Active weather data, CID-referenced |
| Cold | Filecoin | Minutes | Historical datasets with storage deals |
| Archive | Arweave | Minutes | Permanent climate records |

## Resilience Extensions

- Provenance-chain enforcement is validated before acceptance (`storage-providers#9`).
- Invalid or synthetic-suspicious artifacts are routed into quarantine metadata flow (`storage-providers#7`).
- Public-interest classes can be replicated with explicit minimum replica SLA tracking (`storage-providers#10`).

## Development

```bash
cargo check
cargo test
cargo build
```
