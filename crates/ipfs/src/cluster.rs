use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::StorageError;

/// A peer node in the IPFS cluster (weather station network).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterPeer {
    /// Unique identifier for the peer.
    pub id: String,
    /// Multiaddr or hostname of the peer.
    pub address: String,
    /// Geographic latitude of the peer node.
    pub latitude: f64,
    /// Geographic longitude of the peer node.
    pub longitude: f64,
    /// Capabilities advertised by this peer (e.g. "pin", "relay", "gateway").
    pub capabilities: Vec<String>,
}

impl ClusterPeer {
    /// Validate that coordinates are within valid ranges and not NaN/infinite.
    pub fn validate(&self) -> Result<(), StorageError> {
        if !self.latitude.is_finite() || self.latitude < -90.0 || self.latitude > 90.0 {
            return Err(StorageError::ConfigError(format!(
                "invalid latitude {}: must be between -90 and 90",
                self.latitude
            )));
        }
        if !self.longitude.is_finite() || self.longitude < -180.0 || self.longitude > 180.0 {
            return Err(StorageError::ConfigError(format!(
                "invalid longitude {}: must be between -180 and 180",
                self.longitude
            )));
        }
        Ok(())
    }
}

/// Configuration for an IPFS cluster deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsClusterConfig {
    /// Name of the cluster.
    pub cluster_name: String,
    /// Secret shared among cluster peers for authentication.
    pub cluster_secret: Option<String>,
    /// Desired replication factor for pinned content.
    pub replication_factor: u32,
}

impl Default for IpfsClusterConfig {
    fn default() -> Self {
        Self {
            cluster_name: std::env::var("IPFS_CLUSTER_NAME")
                .unwrap_or_else(|_| "wrfcoin-weather".to_string()),
            cluster_secret: std::env::var("IPFS_CLUSTER_SECRET").ok(),
            replication_factor: 3,
        }
    }
}

/// Manages a set of IPFS cluster peers.
#[derive(Debug)]
pub struct ClusterManager {
    config: IpfsClusterConfig,
    peers: Vec<ClusterPeer>,
}

impl ClusterManager {
    /// Create a new cluster manager with the given configuration.
    ///
    /// Returns an error if `cluster_secret` is `Some("")` (fail-closed on empty secrets).
    pub fn new(config: IpfsClusterConfig) -> Result<Self, StorageError> {
        if let Some(ref secret) = config.cluster_secret {
            if secret.is_empty() {
                return Err(StorageError::ConfigError(
                    "IPFS cluster secret is configured but empty — refusing to proceed".to_string(),
                ));
            }
        }
        Ok(Self {
            config,
            peers: Vec::new(),
        })
    }

    /// Return the cluster secret, if configured.
    pub fn cluster_secret(&self) -> Option<&str> {
        self.config.cluster_secret.as_deref()
    }

    /// Return the configured cluster name.
    pub fn cluster_name(&self) -> &str {
        &self.config.cluster_name
    }

    /// Build auth headers for cluster API calls.
    /// Returns a tuple of (header_name, header_value) when a secret is configured.
    pub fn auth_header(&self) -> Option<(&'static str, String)> {
        self.config
            .cluster_secret
            .as_ref()
            .map(|secret| ("Authorization", format!("Bearer {secret}")))
    }

    /// Add a peer to the cluster.
    pub fn add_peer(&mut self, peer: ClusterPeer) -> Result<(), StorageError> {
        peer.validate()?;
        self.peers.push(peer);
        Ok(())
    }

    /// Remove a peer by its identifier. Returns `true` if the peer was found and removed.
    pub fn remove_peer(&mut self, peer_id: &str) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| p.id != peer_id);
        self.peers.len() < before
    }

    /// Return a slice of all known peers.
    pub fn peers(&self) -> &[ClusterPeer] {
        &self.peers
    }

    /// Return the configured replication factor.
    pub fn replication_factor(&self) -> u32 {
        self.config.replication_factor
    }

    /// Select the best peers for replicating content based on geographic proximity.
    ///
    /// Returns up to `replication_factor` peers sorted by distance from the given coordinates.
    pub fn select_peers_near(&self, latitude: f64, longitude: f64) -> Vec<&ClusterPeer> {
        let mut peers_with_dist: Vec<(&ClusterPeer, f64)> = self
            .peers
            .iter()
            .map(|p| {
                let dlat = p.latitude - latitude;
                let dlon = p.longitude - longitude;
                let dist = (dlat * dlat + dlon * dlon).sqrt();
                (p, dist)
            })
            .collect();

        peers_with_dist.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        peers_with_dist
            .into_iter()
            .take(self.config.replication_factor as usize)
            .map(|(peer, _)| peer)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, lat: f64, lon: f64) -> ClusterPeer {
        ClusterPeer {
            id: id.to_string(),
            address: format!("/ip4/127.0.0.1/tcp/4001/p2p/{}", id),
            latitude: lat,
            longitude: lon,
            capabilities: vec!["pin".to_string()],
        }
    }

    #[test]
    fn test_add_and_list_peers() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        cm.add_peer(peer("peer-1", 40.0, -74.0)).unwrap();
        cm.add_peer(peer("peer-2", 51.0, 0.0)).unwrap();
        assert_eq!(cm.peers().len(), 2);
    }

    #[test]
    fn test_remove_peer() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        cm.add_peer(peer("peer-1", 0.0, 0.0)).unwrap();
        assert!(cm.remove_peer("peer-1"));
        assert_eq!(cm.peers().len(), 0);
        assert!(!cm.remove_peer("nonexistent"));
    }

    #[test]
    fn test_select_peers_near_sorts_by_distance() {
        let config = IpfsClusterConfig { replication_factor: 2, ..Default::default() };
        let mut cm = ClusterManager::new(config).unwrap();
        cm.add_peer(peer("far", 90.0, 90.0)).unwrap();
        cm.add_peer(peer("near", 1.0, 1.0)).unwrap();
        cm.add_peer(peer("medium", 10.0, 10.0)).unwrap();

        let selected = cm.select_peers_near(0.0, 0.0);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, "near");
        assert_eq!(selected[1].id, "medium");
    }

    #[test]
    fn test_select_peers_empty_cluster() {
        let cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        let selected = cm.select_peers_near(0.0, 0.0);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_replication_factor() {
        let config = IpfsClusterConfig { replication_factor: 5, ..Default::default() };
        let cm = ClusterManager::new(config).unwrap();
        assert_eq!(cm.replication_factor(), 5);
    }

    #[test]
    fn test_cluster_rejects_empty_secret() {
        let config = IpfsClusterConfig {
            cluster_secret: Some("".to_string()),
            ..Default::default()
        };
        assert!(ClusterManager::new(config).is_err());
    }

    #[test]
    fn test_cluster_accepts_valid_secret() {
        let config = IpfsClusterConfig {
            cluster_secret: Some("my-secret".to_string()),
            ..Default::default()
        };
        let cm = ClusterManager::new(config).unwrap();
        assert_eq!(cm.cluster_secret(), Some("my-secret"));
    }

    #[test]
    fn test_cluster_auth_header_when_secret_set() {
        let config = IpfsClusterConfig {
            cluster_secret: Some("my-secret".to_string()),
            ..Default::default()
        };
        let cm = ClusterManager::new(config).unwrap();
        let (name, value) = cm.auth_header().unwrap();
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer my-secret");
    }

    #[test]
    fn test_cluster_name_from_config() {
        let config = IpfsClusterConfig {
            cluster_name: "my-custom-cluster".to_string(),
            ..Default::default()
        };
        let cm = ClusterManager::new(config).unwrap();
        assert_eq!(cm.cluster_name(), "my-custom-cluster");
    }

    #[test]
    fn test_default_cluster_name() {
        // When IPFS_CLUSTER_NAME env var is not set, falls back to "wrfcoin-weather"
        let config = IpfsClusterConfig::default();
        // The default may be overridden by env var in CI, so just check it's non-empty
        assert!(!config.cluster_name.is_empty());
    }

    #[test]
    fn test_cluster_auth_header_when_no_secret() {
        let config = IpfsClusterConfig {
            cluster_secret: None,
            ..Default::default()
        };
        let cm = ClusterManager::new(config).unwrap();
        assert!(cm.auth_header().is_none());
    }

    #[test]
    fn test_add_peer_rejects_invalid_latitude() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        let result = cm.add_peer(peer("bad-lat", 95.0, 0.0));
        assert!(result.is_err());
        assert_eq!(cm.peers().len(), 0);
    }

    #[test]
    fn test_add_peer_rejects_invalid_longitude() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        let result = cm.add_peer(peer("bad-lon", 0.0, 200.0));
        assert!(result.is_err());
        assert_eq!(cm.peers().len(), 0);
    }

    #[test]
    fn test_add_peer_rejects_nan_coordinates() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        let result = cm.add_peer(peer("nan", f64::NAN, 0.0));
        assert!(result.is_err());
    }

    #[test]
    fn test_add_peer_rejects_infinite_coordinates() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        let result = cm.add_peer(peer("inf", f64::INFINITY, 0.0));
        assert!(result.is_err());
    }

    #[test]
    fn test_add_peer_accepts_boundary_values() {
        let mut cm = ClusterManager::new(IpfsClusterConfig::default()).unwrap();
        cm.add_peer(peer("north-pole", 90.0, 180.0)).unwrap();
        cm.add_peer(peer("south-pole", -90.0, -180.0)).unwrap();
        assert_eq!(cm.peers().len(), 2);
    }
}
