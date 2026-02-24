use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::{ContentId, DataTier};

/// Status of a migration job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationStatus {
    /// Job is queued and waiting to execute.
    Pending,
    /// Job is currently in progress.
    InProgress,
    /// Job completed successfully.
    Completed,
    /// Job failed.
    Failed,
}

/// A job representing the migration of content between storage tiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationJob {
    /// Content to be migrated.
    pub content_id: ContentId,
    /// Source tier.
    pub from_tier: DataTier,
    /// Destination tier.
    pub to_tier: DataTier,
    /// Current status of the migration.
    pub status: MigrationStatus,
}

/// Schedules and tracks migration jobs between storage tiers.
#[derive(Debug)]
pub struct MigrationScheduler {
    pending_jobs: VecDeque<MigrationJob>,
    completed_jobs: Vec<MigrationJob>,
}

impl MigrationScheduler {
    /// Create a new migration scheduler.
    pub fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            completed_jobs: Vec::new(),
        }
    }

    /// Schedule a new migration job.
    pub fn schedule(&mut self, content_id: ContentId, from_tier: DataTier, to_tier: DataTier) {
        self.pending_jobs.push_back(MigrationJob {
            content_id,
            from_tier,
            to_tier,
            status: MigrationStatus::Pending,
        });
    }

    /// Take the next pending job from the queue and mark it as in-progress.
    ///
    /// Returns `None` if there are no pending jobs.
    pub fn execute_next(&mut self) -> Option<MigrationJob> {
        if let Some(mut job) = self.pending_jobs.pop_front() {
            job.status = MigrationStatus::InProgress;
            Some(job)
        } else {
            None
        }
    }

    /// Mark a job as completed and archive it.
    pub fn complete(&mut self, mut job: MigrationJob) {
        job.status = MigrationStatus::Completed;
        self.completed_jobs.push(job);
    }

    /// Mark a job as failed and archive it.
    pub fn fail(&mut self, mut job: MigrationJob) {
        job.status = MigrationStatus::Failed;
        self.completed_jobs.push(job);
    }

    /// Get the status of a migration by content ID.
    ///
    /// Checks pending jobs first, then completed jobs.
    pub fn get_status(&self, content_id: &ContentId) -> Option<&MigrationStatus> {
        // Check pending queue.
        if let Some(job) = self
            .pending_jobs
            .iter()
            .find(|j| j.content_id == *content_id)
        {
            return Some(&job.status);
        }

        // Check completed/failed archive.
        if let Some(job) = self
            .completed_jobs
            .iter()
            .rev()
            .find(|j| j.content_id == *content_id)
        {
            return Some(&job.status);
        }

        None
    }

    /// Return the number of pending jobs.
    pub fn pending_count(&self) -> usize {
        self.pending_jobs.len()
    }

    /// Return the number of completed (including failed) jobs.
    pub fn completed_count(&self) -> usize {
        self.completed_jobs.len()
    }
}

impl Default for MigrationScheduler {
    fn default() -> Self {
        Self::new()
    }
}
