use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrComment {
    pub id: String,
    pub repository: String,
    pub pr_number: i64,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_review_comment: bool,
    pub review_state: Option<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    NewComment,
    NewCommit,
    NewCistatus,
    NewReviewStatus,
    NewPullRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrPerspective {
    MyPr,
    TrackedPr,
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
    pub comments: Vec<PrComment>,
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

    pub fn is_mine(&self, current_user: &str) -> bool {
        !current_user.is_empty() && self.author.eq_ignore_ascii_case(current_user)
    }

    pub fn perspective(&self, current_user: &str) -> PrPerspective {
        if self.is_mine(current_user) {
            PrPerspective::MyPr
        } else {
            PrPerspective::TrackedPr
        }
    }

    pub fn display_string(&self) -> String {
        format!(
            "{} {} : {}/{}",
            self.author, self.title, self.repository, self.number
        )
    }

    pub fn all_changes(&self) -> Vec<ChangeKind> {
        let Some(last_ack) = self.last_acknowledged_at else {
            return vec![ChangeKind::NewPullRequest];
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

    pub fn meaningful_changes(&self, current_user: &str) -> Vec<ChangeKind> {
        let perspective = self.perspective(current_user);

        self.all_changes()
            .into_iter()
            .filter(|change| match perspective {
                PrPerspective::MyPr => !matches!(change, ChangeKind::NewCommit),
                PrPerspective::TrackedPr => true,
            })
            .collect()
    }

    pub fn is_acknowledged_for_user(&self, current_user: &str) -> bool {
        self.last_acknowledged_at.is_some() && self.meaningful_changes(current_user).is_empty()
    }

    pub fn should_notify_on_changes(&self, current_user: &str) -> bool {
        match self.perspective(current_user) {
            PrPerspective::MyPr => self.should_notify_for_my_pr(current_user),
            PrPerspective::TrackedPr => self.should_notify_for_tracked_pr(current_user),
        }
    }

    fn should_notify_for_my_pr(&self, current_user: &str) -> bool {
        debug_assert!(matches!(
            self.perspective(current_user),
            PrPerspective::MyPr
        ));

        self.meaningful_changes(current_user)
            .into_iter()
            .any(|change| {
                matches!(
                    change,
                    ChangeKind::NewComment | ChangeKind::NewCistatus | ChangeKind::NewReviewStatus
                )
            })
    }

    fn should_notify_for_tracked_pr(&self, current_user: &str) -> bool {
        debug_assert!(matches!(
            self.perspective(current_user),
            PrPerspective::TrackedPr
        ));

        !self.meaningful_changes(current_user).is_empty()
    }

    pub fn updates_since_last_ack(&self, current_user: &str) -> String {
        let changes = self.meaningful_changes(current_user);

        if changes.is_empty() {
            return "  ".to_string();
        }

        let mut updates = String::from("  ");
        for change in changes {
            match change {
                ChangeKind::NewComment => updates.push_str("New Comment | "),
                ChangeKind::NewCommit => updates.push_str("New Commits | "),
                ChangeKind::NewCistatus => updates.push_str("CI Status Changed | "),
                ChangeKind::NewReviewStatus => updates.push_str("Review Status Changed | "),
                ChangeKind::NewPullRequest => updates.push_str("New PR | "),
            }
        }

        updates
    }

    pub fn user_is_involved(&self, current_user: &str) -> bool {
        if current_user.is_empty() {
            return false;
        }

        if self.author.eq_ignore_ascii_case(current_user) {
            return true;
        }

        self.requested_reviewers
            .iter()
            .any(|reviewer| reviewer.eq_ignore_ascii_case(current_user))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0)
            .single()
            .expect("valid timestamp")
    }

    fn author() -> String {
        "octocat".to_string()
    }

    fn not_author() -> String {
        "not-octocat".to_string()
    }

    #[derive(Debug, Clone, Copy)]
    enum TestPrEvent {
        Comment,
        Commit,
        CiStatus,
        ReviewStatus,
        Ack,
    }

    fn build_pull_request(events: &[TestPrEvent]) -> PullRequest {
        let base_time = timestamp(1);
        let mut pr = PullRequest {
            number: 42,
            title: "Improve all_changes tests".to_string(),
            repository: "owner/repo".to_string(),
            author: "octocat".to_string(),
            head_sha: "abc123".to_string(),
            draft: false,
            created_at: base_time,
            updated_at: base_time,
            ci_status: CiStatus::Pending,
            last_comment_at: base_time,
            last_commit_at: base_time,
            last_ci_status_update_at: base_time,
            approval_status: ApprovalStatus::None,
            last_review_status_update_at: base_time,
            last_acknowledged_at: None,
            requested_reviewers: vec![],
            user_has_reviewed: false,
            comments: vec![],
        };

        for (index, event) in events.iter().enumerate() {
            let at = timestamp(index as i64 + 2);
            pr.updated_at = at;

            match event {
                TestPrEvent::Comment => pr.last_comment_at = at,
                TestPrEvent::Commit => pr.last_commit_at = at,
                TestPrEvent::CiStatus => pr.last_ci_status_update_at = at,
                TestPrEvent::ReviewStatus => pr.last_review_status_update_at = at,
                TestPrEvent::Ack => pr.last_acknowledged_at = Some(at),
            }
        }

        pr
    }

    #[test]
    fn should_notify_returns_true() {
        assert!(build_pull_request(&[]).should_notify_on_changes("foo"));
    }

    #[test]
    fn notify_returns_false_for_new_pr_by_user() {
        let pr = build_pull_request(&[]);

        assert!(!pr.should_notify_on_changes(&author()));
    }

    #[test]
    fn notify_returns_true_for_new_pr_by_other_author() {
        let pr = build_pull_request(&[]);

        assert!(pr.should_notify_on_changes(&not_author()));
    }

    #[test]
    fn notify_returns_false_for_commit_on_authors_pr() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(!pr.should_notify_on_changes(&author()));
    }

    #[test]
    fn notify_returns_true_for_commit_on_other_prs() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.should_notify_on_changes(&not_author()));
    }

    #[test]
    fn meaningful_changes_ignores_new_commit_for_my_pr() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.meaningful_changes(&author()).is_empty());
    }

    #[test]
    fn meaningful_changes_keeps_new_commit_for_tracked_pr() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert_eq!(
            pr.meaningful_changes(&not_author()),
            vec![ChangeKind::NewCommit]
        );
    }

    #[test]
    fn is_acknowledged_for_user_stays_true_for_my_new_commit() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_resets_for_my_new_comment() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Comment]);

        assert!(!pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_resets_for_tracked_pr_new_commit() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(!pr.is_acknowledged_for_user(&not_author()));
    }

    #[test]
    fn updates_since_last_ack_hides_my_commit_only_changes() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert_eq!(pr.updates_since_last_ack(&author()), "  ");
    }

    #[test]
    fn updates_since_last_ack_shows_new_pr_for_my_pr() {
        let pr = build_pull_request(&[]);

        assert_eq!(pr.updates_since_last_ack(&author()), "  New PR | ");
    }

    #[test]
    fn all_changes_returns_new_pull_request_when_never_acknowledged() {
        let pr = build_pull_request(&[TestPrEvent::Commit, TestPrEvent::Comment]);

        let changes = pr.all_changes();

        assert!(matches!(changes.as_slice(), [ChangeKind::NewPullRequest]));
    }

    #[test]
    fn all_changes_returns_empty_when_nothing_new_since_ack() {
        let pr = build_pull_request(&[
            TestPrEvent::Comment,
            TestPrEvent::Commit,
            TestPrEvent::CiStatus,
            TestPrEvent::ReviewStatus,
            TestPrEvent::Ack,
        ]);

        let changes = pr.all_changes();

        assert!(changes.is_empty());
    }

    #[test]
    fn all_changes_returns_only_changes_after_ack_in_expected_order() {
        let pr = build_pull_request(&[
            TestPrEvent::Comment,
            TestPrEvent::Ack,
            TestPrEvent::Commit,
            TestPrEvent::CiStatus,
            TestPrEvent::ReviewStatus,
        ]);

        let changes = pr.all_changes();

        assert!(matches!(
            changes.as_slice(),
            [
                ChangeKind::NewCommit,
                ChangeKind::NewCistatus,
                ChangeKind::NewReviewStatus
            ]
        ));
    }

    #[test]
    fn user_is_involved_returns_true_for_author() {
        let pr = build_pull_request(&[]);

        assert!(pr.user_is_involved(&author()));
    }

    #[test]
    fn user_is_involved_returns_true_for_requested_reviewer() {
        let mut pr = build_pull_request(&[]);
        pr.requested_reviewers = vec!["reviewer".to_string()];

        assert!(pr.user_is_involved("reviewer"));
    }

    #[test]
    fn user_is_involved_returns_false_for_unrelated_user() {
        let mut pr = build_pull_request(&[]);
        pr.requested_reviewers = vec!["reviewer".to_string()];

        assert!(!pr.user_is_involved("someone-else"));
    }

    #[test]
    fn user_is_involved_is_case_insensitive() {
        let mut pr = build_pull_request(&[]);
        pr.author = "OctoCat".to_string();
        pr.requested_reviewers = vec!["ReVieWer".to_string()];

        assert!(pr.user_is_involved("octocat"));
        assert!(pr.user_is_involved("reviewer"));
    }
}
