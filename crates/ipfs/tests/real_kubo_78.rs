//! Real-Kubo round trip for storage-providers#78. Skipped (passes, prints a note) unless `KUBO_API`
//! names a Kubo HTTP API, e.g. `KUBO_API=http://127.0.0.1:15001`. Run it through
//! `scripts/real-kubo-roundtrip.sh`, which starts a disposable offline Kubo, runs phase `add`,
//! restarts the container, then runs phase `cat`.
//!
//! Phases (`KUBO_PHASE`, default `all`): `add` stores the blob and records its CID in `KUBO_CID_FILE`;
//! `cat` retrieves it through a FRESH provider (empty cache, so the bytes come from Kubo) and checks
//! intact bytes verify and corrupt bytes are refused. A real Kubo cannot be made to serve wrong bytes
//! for a CID, so corruption is exercised through `verify_retrieved`, which uses the real
//! `add?only-hash=true`.
use wrfcoin_ipfs::{IpfsConfig, IpfsStorageProvider};
use wrfcoin_storage_common::{ContentId, Metadata, StorageError, StorageProvider};

/// More than one default UnixFS chunk (256 KiB), so the CID is a dag-pb root, not a raw digest.
fn blob() -> Vec<u8> {
    (0..300_000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect()
}

fn provider(api: &str) -> IpfsStorageProvider {
    IpfsStorageProvider::try_new(IpfsConfig::unauthenticated(api.to_string())).expect("kubo client")
}

fn metadata() -> Metadata {
    Metadata {
        content_type: "application/octet-stream".into(),
        size: blob().len() as u64,
        timestamp: 0,
        tags: std::collections::HashMap::new(),
    }
}

#[tokio::test]
async fn real_kubo_round_trip_78() {
    let Ok(api) = std::env::var("KUBO_API") else {
        eprintln!("real_kubo_78: KUBO_API not set, skipping (see scripts/real-kubo-roundtrip.sh)");
        return;
    };
    let phase = std::env::var("KUBO_PHASE").unwrap_or_else(|_| "all".into());
    let cid_file =
        std::env::var("KUBO_CID_FILE").unwrap_or_else(|_| "/tmp/real-kubo-78.cid".into());
    let data = blob();

    if phase == "add" || phase == "all" {
        let p = provider(&api);
        let id = p
            .store(&data, &metadata())
            .await
            .expect("store via real Kubo");
        eprintln!("ADD cid={} bytes={}", id.hash, data.len());
        assert_ne!(
            id.hash,
            wrfcoin_storage_common::hashing::ipfs_content_id(&data).hash,
            "a multi-chunk UnixFS CID must not be the raw digest"
        );
        // Reimporting the same bytes yields the same CID.
        p.verify_retrieved(&id.hash, &data)
            .await
            .expect("intact content re-imports to the same CID");
        eprintln!("REIMPORT intact cid={} same=true", id.hash);
        std::fs::write(&cid_file, &id.hash).expect("write cid file");
    }

    if phase == "cat" || phase == "all" {
        let hash = std::fs::read_to_string(&cid_file)
            .expect("read cid file")
            .trim()
            .to_string();
        let id = ContentId {
            provider: "ipfs".into(),
            hash: hash.clone(),
        };
        let fresh = provider(&api); // empty cache: the bytes must come from Kubo
        let got = fresh
            .retrieve(&id)
            .await
            .expect("retrieve via real Kubo (cat + verify)");
        assert_eq!(got, data, "retrieved bytes differ");
        eprintln!("CAT cid={hash} bytes={} verified=true", got.len());

        let mut corrupt = data.clone();
        corrupt[150_000] ^= 0x01;
        let err = fresh
            .verify_retrieved(&hash, &corrupt)
            .await
            .expect_err("corrupt content must be refused");
        assert!(matches!(err, StorageError::IntegrityMismatch(_)), "{err:?}");
        eprintln!("REIMPORT corrupt(1 bit) refused: {err}");
        let truncated = &data[..data.len() - 1];
        assert!(matches!(
            fresh.verify_retrieved(&hash, truncated).await,
            Err(StorageError::IntegrityMismatch(_))
        ));
        eprintln!("REIMPORT truncated refused");
    }
}
