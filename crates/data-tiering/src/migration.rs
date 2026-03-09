use std::collections::{HashMap, VecDeque};

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
    /// Number of attempts so far.
    pub attempt: u32,
    /// Maximum number of retry attempts allowed.
    pub max_attempts: u32,
}

/// Schedules and tracks migration jobs between storage tiers.
#[derive(Debug)]
pub struct MigrationScheduler {
    pending_jobs: VecDeque<MigrationJob>,
    completed_jobs: Vec<MigrationJob>,
    pending_counts_by_content: HashMap<ContentId, usize>,
    completed_status_by_content: HashMap<ContentId, MigrationStatus>,
    status_by_content: HashMap<ContentId, MigrationStatus>,
    default_max_attempts: u32,
}

impl MigrationScheduler {
    /// Create a new migration scheduler with a default max of 3 attempts.
    pub fn new() -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            completed_jobs: Vec::new(),
            pending_counts_by_content: HashMap::new(),
            completed_status_by_content: HashMap::new(),
            status_by_content: HashMap::new(),
            default_max_attempts: 3,
        }
    }

    /// Create a scheduler with a custom max-attempts default.
    pub fn with_max_attempts(max_attempts: u32) -> Self {
        Self {
            pending_jobs: VecDeque::new(),
            completed_jobs: Vec::new(),
            pending_counts_by_content: HashMap::new(),
            completed_status_by_content: HashMap::new(),
            status_by_content: HashMap::new(),
            default_max_attempts: max_attempts,
        }
    }

    /// Schedule a new migration job.
    pub fn schedule(&mut self, content_id: ContentId, from_tier: DataTier, to_tier: DataTier) {
        *self
            .pending_counts_by_content
            .entry(content_id.clone())
            .or_insert(0) += 1;
        self.status_by_content
            .insert(content_id.clone(), MigrationStatus::Pending);
        self.pending_jobs.push_back(MigrationJob {
            content_id,
            from_tier,
            to_tier,
            status: MigrationStatus::Pending,
            attempt: 0,
            max_attempts: self.default_max_attempts,
        });
    }

    /// Take the next pending job from the queue and mark it as in-progress.
    ///
    /// Returns `None` if there are no pending jobs.
    pub fn execute_next(&mut self) -> Option<MigrationJob> {
        if let Some(mut job) = self.pending_jobs.pop_front() {
            let content_id = job.content_id.clone();
            if let Some(count) = self.pending_counts_by_content.get_mut(&content_id) {
                *count -= 1;
                if *count == 0 {
                    self.pending_counts_by_content.remove(&content_id);
                    if let Some(status) = self.completed_status_by_content.get(&content_id) {
                        self.status_by_content.insert(content_id, status.clone());
                    } else {
                        self.status_by_content.remove(&content_id);
                    }
                }
            }
            job.status = MigrationStatus::InProgress;
            job.attempt += 1;
            Some(job)
        } else {
            None
        }
    }

    /// Mark a job as completed and archive it.
    pub fn complete(&mut self, mut job: MigrationJob) {
        job.status = MigrationStatus::Completed;
        self.completed_status_by_content
            .insert(job.content_id.clone(), MigrationStatus::Completed);
        if !self.pending_counts_by_content.contains_key(&job.content_id) {
            self.status_by_content
                .insert(job.content_id.clone(), MigrationStatus::Completed);
        }
        self.completed_jobs.push(job);
    }

    /// Mark a job as failed and archive it.
    pub fn fail(&mut self, mut job: MigrationJob) {
        job.status = MigrationStatus::Failed;
        self.completed_status_by_content
            .insert(job.content_id.clone(), MigrationStatus::Failed);
        if !self.pending_counts_by_content.contains_key(&job.content_id) {
            self.status_by_content
                .insert(job.content_id.clone(), MigrationStatus::Failed);
        }
        self.completed_jobs.push(job);
    }

    /// Retry a failed job. Returns `true` if the job was re-enqueued,
    /// `false` if max attempts have been exhausted.
    pub fn retry(&mut self, job: MigrationJob) -> bool {
        if job.attempt >= job.max_attempts {
            return false;
        }
        let mut retried = job;
        retried.status = MigrationStatus::Pending;
        *self
            .pending_counts_by_content
            .entry(retried.content_id.clone())
            .or_insert(0) += 1;
        self.status_by_content
            .insert(retried.content_id.clone(), MigrationStatus::Pending);
        self.pending_jobs.push_back(retried);
        true
    }

    /// Get the status of a migration by content ID.
    ///
    /// Checks pending jobs first, then completed jobs.
    pub fn get_status(&self, content_id: &ContentId) -> Option<&MigrationStatus> {
        self.status_by_content.get(content_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cid(hash: &str) -> ContentId {
        ContentId {
            provider: "ipfs".to_string(),
            hash: hash.to_string(),
        }
    }

    #[test]
    fn test_schedule_and_pending_count() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        ms.schedule(cid("b"), DataTier::Warm, DataTier::Cold);
        assert_eq!(ms.pending_count(), 2);
    }

    #[test]
    fn test_execute_next() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        let job = ms.execute_next().unwrap();
        assert_eq!(job.status, MigrationStatus::InProgress);
        assert_eq!(job.content_id, cid("a"));
        assert_eq!(ms.pending_count(), 0);
    }

    #[test]
    fn test_execute_next_empty() {
        let mut ms = MigrationScheduler::new();
        assert!(ms.execute_next().is_none());
    }

    #[test]
    fn test_complete_job() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Cold);
        let job = ms.execute_next().unwrap();
        ms.complete(job);
        assert_eq!(ms.completed_count(), 1);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Completed));
    }

    #[test]
    fn test_fail_job() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Warm, DataTier::Archive);
        let job = ms.execute_next().unwrap();
        ms.fail(job);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Failed));
    }

    #[test]
    fn test_get_status_pending() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Cold);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Pending));
    }

    #[test]
    fn test_get_status_unknown() {
        let ms = MigrationScheduler::new();
        assert_eq!(ms.get_status(&cid("unknown")), None);
    }

    #[test]
    fn test_status_none_while_in_progress_without_history() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        let _job = ms.execute_next().unwrap();
        assert_eq!(ms.get_status(&cid("a")), None);
    }

    #[test]
    fn test_status_prefers_pending_over_completed() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        let job = ms.execute_next().unwrap();
        ms.complete(job);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Completed));

        ms.schedule(cid("a"), DataTier::Warm, DataTier::Cold);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Pending));
    }

    #[test]
    fn test_status_restores_completed_after_pending_drained() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        let job = ms.execute_next().unwrap();
        ms.complete(job);

        ms.schedule(cid("a"), DataTier::Warm, DataTier::Cold);
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Pending));
        let _in_progress = ms.execute_next().unwrap();
        assert_eq!(ms.get_status(&cid("a")), Some(&MigrationStatus::Completed));
    }

    #[test]
    fn test_pending_count_index_consistency_for_duplicates() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("dup"), DataTier::Hot, DataTier::Warm);
        ms.schedule(cid("dup"), DataTier::Warm, DataTier::Cold);

        let _job = ms.execute_next().unwrap();
        assert_eq!(ms.get_status(&cid("dup")), Some(&MigrationStatus::Pending));

        let _job = ms.execute_next().unwrap();
        assert_eq!(ms.get_status(&cid("dup")), None);
    }

    #[test]
    fn test_fifo_order() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("first"), DataTier::Hot, DataTier::Warm);
        ms.schedule(cid("second"), DataTier::Warm, DataTier::Cold);
        let job1 = ms.execute_next().unwrap();
        assert_eq!(job1.content_id, cid("first"));
        let job2 = ms.execute_next().unwrap();
        assert_eq!(job2.content_id, cid("second"));
    }

    #[test]
    fn test_retry_increments_attempt() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Cold);
        let job = ms.execute_next().unwrap();
        assert_eq!(job.attempt, 1);
        assert!(ms.retry(job));
        let retried = ms.execute_next().unwrap();
        assert_eq!(retried.attempt, 2);
    }

    #[test]
    fn test_retry_max_attempts_exceeded() {
        let mut ms = MigrationScheduler::with_max_attempts(2);
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Cold);
        // Attempt 1
        let job = ms.execute_next().unwrap();
        assert!(ms.retry(job));
        // Attempt 2
        let job = ms.execute_next().unwrap();
        assert_eq!(job.attempt, 2);
        // Attempt 3 should be denied (max_attempts is 2)
        assert!(!ms.retry(job));
    }

    #[test]
    fn test_with_max_attempts_constructor() {
        let mut ms = MigrationScheduler::with_max_attempts(5);
        ms.schedule(cid("a"), DataTier::Warm, DataTier::Archive);
        let job = ms.execute_next().unwrap();
        assert_eq!(job.max_attempts, 5);
    }

    #[test]
    fn test_default_max_attempts() {
        let mut ms = MigrationScheduler::new();
        ms.schedule(cid("a"), DataTier::Hot, DataTier::Warm);
        let job = ms.execute_next().unwrap();
        assert_eq!(job.max_attempts, 3);
    }
}
