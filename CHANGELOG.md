# Changelog

The format follows Keep a Changelog. This launch entry describes source capabilities, not a signed binary release or deployment.

## [Unreleased]

### Launch preparation

- Clarify local storage capabilities and unavailable network integrations. ([#90](https://github.com/wrfcoin/storage-providers/pull/90))
- Record local test evidence without implying deployed integration. ([#91](https://github.com/wrfcoin/storage-providers/pull/91))
- Add the canonical public-testnet disclaimer. ([#92](https://github.com/wrfcoin/storage-providers/pull/92))
- Describe the receipt archiver crate in package metadata. ([#93](https://github.com/wrfcoin/storage-providers/pull/93))


## 2026-10-22 public testnet launch

### Added

- IPFS-only local storage-node profile with health and stats endpoints.
- Content-addressed retrieval verification and tiering libraries.
- Explicit unavailable responses for providers outside the testnet profile.
