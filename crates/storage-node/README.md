# Storage Daemon

HTTP server daemon exposing the StorageProvider trait operations over REST endpoints.

## Overview

The storage daemon provides operators with a runnable service that exposes IPFS, Filecoin, and Arweave storage capabilities through a standardized REST API. This enables weather data to be stored and retrieved across multiple decentralized storage networks without requiring direct library integration.

## Features

- **Multi-provider support**: IPFS, Filecoin, Arweave (configurable)
- **REST API**: Store, retrieve, delete, and check existence of content
- **Health checks**: Monitor health and statistics of all configured providers
- **Configurable defaults**: Choose which provider to use by default
- **Content addressing**: Full support for content-addressed storage (CID/IPFS, Arweave, Filecoin)

## Configuration

The daemon reads environment variables to determine which storage providers to initialize:

- `STORAGE_PROVIDERS`: Comma-separated list of providers (default: `ipfs`)
- `DEFAULT_PROVIDER`: Default provider for operations (default: `ipfs`)
- `IPFS_API_TOKEN`: Bearer token for IPFS API (optional)
- `RUST_LOG`: Log level (default: `wrfcoin_storage_daemon=debug`)

### Provider-Specific Config

Configuration is read from environment variables or defaults to sensible values:

**IPFS** (`IpfsConfig`):
- `IPFS_API_URL`: Kubo HTTP API URL (default: `http://localhost:5001`)
- `IPFS_GATEWAY_URL`: IPFS gateway URL (default: `http://localhost:8080`)
- `IPFS_CACHE_CAPACITY`: LRU cache size (default: 10,000)
- `IPFS_CACHE_TTL_SECS`: Cache TTL in seconds (default: 3600)

**Filecoin** (`FilecoinConfig`):
- `FILECOIN_LOTUS_URL`: Lotus JSON-RPC URL (default: `http://localhost:1234/rpc/v0`)
- `FILECOIN_WALLET_ADDRESS`: Wallet address for deal payments (optional)

**Arweave** (`ArweaveConfig`):
- `ARWEAVE_GATEWAY_URL`: Gateway URL (default: `https://arweave.net`)
- `ARWEAVE_WALLET_PATH`: Path to JWK wallet file (optional)

## Running the Daemon

```bash
# Start with default IPFS configuration
cargo run --bin storage-daemon

# Start with multiple providers
STORAGE_PROVIDERS=ipfs,filecoin,arweave \
DEFAULT_PROVIDER=filecoin \
cargo run --bin storage-daemon

# With custom IPFS endpoint
IPFS_API_URL=http://192.168.1.100:5001 \
cargo run --bin storage-daemon
```

## REST API Endpoints

### Health & Status

**GET /health** - Check health of all providers

```bash
curl http://localhost:7001/health
```

Response:
```json
{
  "status": "healthy",
  "providers": [
    {
      "name": "ipfs",
      "healthy": true,
      "latency_ms": 42,
      "message": null
    }
  ],
  "timestamp": 1712700000
}
```

**GET /stats** - Get aggregate statistics

```bash
curl http://localhost:7001/stats
```

Response:
```json
{
  "providers": [
    {
      "name": "ipfs",
      "stats": {
        "total_stored": 100,
        "total_retrieved": 50,
        "total_bytes": 5242880,
        "cache_hits": 40,
        "cache_misses": 10
      }
    }
  ],
  "timestamp": 1712700000
}
```

### Data Operations

**POST /api/store** - Store data

```bash
curl -X POST http://localhost:7001/api/store \
  -H "Content-Type: application/json" \
  -d '{
    "data": "SGVsbG8gV29ybGQ=",
    "content_type": "text/plain",
    "provider": "ipfs",
    "tags": {
      "domain": "weather",
      "station_id": "US-KORD"
    }
  }'
```

Response (201 Created):
```json
{
  "content_id": {
    "provider": "ipfs",
    "hash": "QmXxxx..."
  },
  "provider": "ipfs",
  "timestamp": 1712700000,
  "size": 11
}
```

**GET /api/retrieve/:id** - Retrieve stored data

Content ID format: `provider:hash` (e.g., `ipfs:QmXxxx...`)

```bash
curl http://localhost:7001/api/retrieve/ipfs:QmXxxx...
```

Returns raw bytes of the stored content.

**GET /api/exists/:id** - Check if content exists

```bash
curl http://localhost:7001/api/exists/ipfs:QmXxxx...
```

Response:
```json
{
  "content_id": "ipfs:QmXxxx...",
  "exists": true,
  "timestamp": 1712700000
}
```

**DELETE /api/delete/:id** - Delete content

```bash
curl -X DELETE http://localhost:7001/api/delete/ipfs:QmXxxx...
```

Response:
```json
{
  "status": "deleted",
  "content_id": "ipfs:QmXxxx...",
  "timestamp": 1712700000
}
```

## Error Handling

The API returns standard HTTP status codes:

- `200 OK` - Success
- `201 Created` - Resource created
- `400 Bad Request` - Invalid input
- `403 Forbidden` - Permission denied
- `404 Not Found` - Content not found
- `402 Payment Required` - Cost exceeded
- `503 Service Unavailable` - Provider unavailable
- `504 Gateway Timeout` - Operation timeout

Error responses include a JSON body:

```json
{
  "error": "content not found: ipfs:QmXxxx...",
  "status": 404
}
```

## Integration with WRFCoin

The daemon integrates seamlessly with the data-tiering system:

- Content stored via the daemon can be referenced by the data-tiering crate
- Metadata tags enable quality-aware routing decisions
- Multi-provider failover is supported through the StorageProvider trait

## Port

Default port: **7001**

## Logging

Structured JSON logging via tracing. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=wrfcoin_storage_daemon=trace cargo run --bin storage-daemon
```
