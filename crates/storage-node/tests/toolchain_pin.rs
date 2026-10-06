//! Toolchain coupling guard.
//!
//! `rust-toolchain.toml` pins what CI checks. What ships is built by infra's
//! `infra/docker/Dockerfile.storage` (`FROM rust:<major.minor>-bookworm@sha256:...`),
//! and that builder never reads this file, so the two can drift silently.
//!
//! Both repos assert the SAME constant, `SHIPPED_BUILDER_RUST_MINOR`: here against
//! the toml channel, and in wrfcoin/infra (`scripts/tests/test_storage_toolchain_pin.py`)
//! against the Dockerfile's rust tag. Bumping one side without the other fails a
//! test. To bump: change the toml, the Dockerfile and BOTH constants together.

/// Must equal the `rust:<major.minor>` tag in infra's Dockerfile.storage.
const SHIPPED_BUILDER_RUST_MINOR: &str = "1.88";

fn toml_channel() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../rust-toolchain.toml");
    let text = std::fs::read_to_string(path).expect("rust-toolchain.toml");
    text.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("channel"))
        .and_then(|rest| rest.split('"').nth(1))
        .expect("channel = \"...\" line")
        .to_string()
}

fn minor_of(channel: &str) -> String {
    channel.split('.').take(2).collect::<Vec<_>>().join(".")
}

#[test]
fn toolchain_channel_matches_shipped_builder_minor() {
    let channel = toml_channel();
    assert_eq!(
        minor_of(&channel),
        SHIPPED_BUILDER_RUST_MINOR,
        "rust-toolchain.toml channel {channel} drifted from the Dockerfile.storage builder; \
         update infra's Dockerfile and both constants together"
    );
}

#[test]
fn minor_of_ignores_patch() {
    assert_eq!(minor_of("1.88.0"), "1.88");
}
