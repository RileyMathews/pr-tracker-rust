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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub repository: String,
    pub author: String,
    pub draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ci_status: CiStatus,
    pub last_comment_at: DateTime<Utc>,
    pub last_commit_at: DateTime<Utc>,
    pub last_ci_status_update_at: DateTime<Utc>,
    pub last_acknowledged_at: Option<DateTime<Utc>>,
    pub requested_reviewers: Vec<String>,
}

impl PullRequest {
    pub fn display_string(&self) -> String {
        format!(
            "{} {} : {}/{}",
            self.author, self.title, self.repository, self.number
        )
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
pub struct User {
    pub access_token: String,
    pub username: String,
}
