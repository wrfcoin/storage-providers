use wrfcoin_storage_common::meteo_compat::{ProvenanceChain, ProvenanceEntry};

#[test]
fn provenance_entry_json_shape_is_pinned() {
    let entry = ProvenanceEntry {
        attester_id: "station-kdlh".into(),
        timestamp: 1_726_128_000,
    };

    assert_eq!(
        serde_json::to_value(entry).unwrap(),
        serde_json::json!({
            "attester_id": "station-kdlh",
            "timestamp": 1_726_128_000_u64,
        })
    );
}

#[test]
fn provenance_chain_roundtrip() {
    let chain = ProvenanceChain::new(vec![ProvenanceEntry {
        attester_id: "station-kdlh".into(),
        timestamp: 1_726_128_000,
    }]);
    let encoded = serde_json::to_string(&chain).unwrap();
    let decoded: ProvenanceChain = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, chain);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&encoded).unwrap(), serde_json::json!([
        { "attester_id": "station-kdlh", "timestamp": 1_726_128_000_u64 }
    ]));
}
