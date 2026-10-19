# Contributing

Read the [README](README.md) for this repository's purpose and technical details.
Follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Build and test

Use the pinned Rust toolchain in rust-toolchain.toml. From the workspace root:

```sh
bash scripts/ci-local.selftest.sh
bash scripts/workflow-gate.selftest.sh
bash scripts/ci-local.sh
```

The workspace gate covers formatting, warnings, tests and the fresh-clone build. The README describes provider-specific commands and the IPFS-only testnet scope.

## Branch and pull request flow

Fork the repository and create a focused branch from current `main`.
Add tests for behavior changes and run the checks above before submitting a pull request.
Describe the change, its motivation, and validation results; use `Refs #N` to link the issue.
Keep unrelated changes separate and wait for maintainer review before merging.
Report vulnerabilities through [SECURITY.md](SECURITY.md).
