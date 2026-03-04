use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiStatus {
    Pending,
    Success,
    Failure,
}

impl CiStatus {
    pub fn as_i64(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Success => 1,
            Self::Failure => 2,
        }
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Success,
            2 => Self::Failure,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    None,
    Approved,
    ChangesRequested,
}

impl ApprovalStatus {
    pub fn as_i64(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Approved => 1,
            Self::ChangesRequested => 2,
        }
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Approved,
            2 => Self::ChangesRequested,
            _ => Self::None,
        }
    }
}

pub enum ChangeKind {
    NewComment,
    NewCommit,
    NewCistatus,
    NewReviewStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub repository: String,
    pub author: String,
    pub head_sha: String,
    pub draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ci_status: CiStatus,
    pub last_comment_at: DateTime<Utc>,
    pub last_commit_at: DateTime<Utc>,
    pub last_ci_status_update_at: DateTime<Utc>,
    pub approval_status: ApprovalStatus,
    pub last_review_status_update_at: DateTime<Utc>,
    pub last_acknowledged_at: Option<DateTime<Utc>>,
    pub requested_reviewers: Vec<String>,
    pub user_has_reviewed: bool,
}

impl PullRequest {
    pub fn is_acknowledged(&self) -> bool {
        let Some(last_ack) = self.last_acknowledged_at else {
            return false;
        };

        self.last_comment_at <= last_ack
            && self.last_commit_at <= last_ack
            && self.last_ci_status_update_at <= last_ack
            && self.last_review_status_update_at <= last_ack
    }

    pub fn display_string(&self) -> String {
        format!(
            "{} {} : {}/{}",
            self.author, self.title, self.repository, self.number
        )
    }

    pub fn all_changes(&self) -> Vec<ChangeKind> {
        let Some(last_ack) = self.last_acknowledged_at else {
            return vec![
                ChangeKind::NewComment,
                ChangeKind::NewCommit,
                ChangeKind::NewCistatus,
                ChangeKind::NewReviewStatus,
            ];
        };

        let mut changes = Vec::new();
        if self.last_comment_at > last_ack {
            changes.push(ChangeKind::NewComment);
        }
        if self.last_commit_at > last_ack {
            changes.push(ChangeKind::NewCommit);
        }
        if self.last_ci_status_update_at > last_ack {
            changes.push(ChangeKind::NewCistatus);
        }
        if self.last_review_status_update_at > last_ack {
            changes.push(ChangeKind::NewReviewStatus);
        }
        changes
    }

    pub fn updates_since_last_ack(&self) -> String {
        if let Some(last_ack) = self.last_acknowledged_at {
            let mut updates = String::from("  ");
            if self.last_comment_at > last_ack {
                updates.push_str("New Comment | ");
            }
            if self.last_commit_at > last_ack {
                updates.push_str("New Commits | ");
            }
            if self.last_ci_status_update_at > last_ack {
                updates.push_str("CI Status Changed | ");
            }
            if self.last_review_status_update_at > last_ack {
                updates.push_str("Review Status Changed | ");
            }
            return updates;
        }

        "  New PR".to_string()
    }

    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/pull/{}",
            self.repository, self.number
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedRepository {
    pub repository: String,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub access_token: String,
    pub username: String,
}
