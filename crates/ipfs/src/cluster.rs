use serde::{Deserialize, Serialize};

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
            cluster_name: "wrfcoin-weather".to_string(),
            cluster_secret: None,
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
    pub fn new(config: IpfsClusterConfig) -> Self {
        Self {
            config,
            peers: Vec::new(),
        }
    }

    /// Add a peer to the cluster.
    pub fn add_peer(&mut self, peer: ClusterPeer) {
        self.peers.push(peer);
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
