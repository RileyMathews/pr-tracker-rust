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

impl std::fmt::Display for CiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pending => "pending",
            Self::Success => "succeeded",
            Self::Failure => "failed",
        })
    }
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
    pub fn repository_name(&self) -> &str {
        self.repository
            .rsplit_once('/')
            .map_or(self.repository.as_str(), |(_, repo_name)| repo_name)
    }

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

    fn ack_display_changes(&self, current_user: &str) -> Vec<ChangeKind> {
        let perspective = self.perspective(current_user);
        let last_ack = self.last_acknowledged_at;

        self.all_changes()
            .into_iter()
            .filter(|change| match change {
                ChangeKind::NewPullRequest => true,
                ChangeKind::NewCommit => match perspective {
                    PrPerspective::MyPr => false,
                    PrPerspective::TrackedPr => {
                        self.commit_changes_are_meaningful_for_user(current_user)
                    }
                },
                ChangeKind::NewCistatus => self.ci_change_is_meaningful(),
                ChangeKind::NewComment => last_ack.is_none_or(|last_ack| {
                    self.has_external_comment_activity_since(last_ack, current_user)
                }),
                ChangeKind::NewReviewStatus => last_ack.is_none_or(|last_ack| {
                    self.has_external_review_activity_since(last_ack, current_user)
                }),
            })
            .collect()
    }

    pub fn is_acknowledged_for_user(&self, current_user: &str) -> bool {
        self.last_acknowledged_at.is_some() && self.ack_display_changes(current_user).is_empty()
    }

    pub fn updates_since_last_ack(&self, current_user: &str) -> String {
        let changes = self.ack_display_changes(current_user);

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

    fn user_is_or_was_involved(&self, current_user: &str) -> bool {
        self.user_is_involved(current_user) || self.user_has_reviewed
    }

    fn commit_changes_are_meaningful_for_user(&self, current_user: &str) -> bool {
        self.user_is_or_was_involved(current_user)
    }

    fn ci_change_is_meaningful(&self) -> bool {
        !matches!(self.ci_status, CiStatus::Pending)
    }

    fn has_external_comment_activity_since(
        &self,
        last_ack: DateTime<Utc>,
        current_user: &str,
    ) -> bool {
        self.comments.iter().any(|comment| {
            comment.updated_at > last_ack && !author_matches_user(&comment.author, current_user)
        })
    }

    fn has_external_review_activity_since(
        &self,
        last_ack: DateTime<Utc>,
        current_user: &str,
    ) -> bool {
        self.comments.iter().any(|comment| {
            comment.is_review_comment
                && comment.updated_at > last_ack
                && !author_matches_user(&comment.author, current_user)
        })
    }

    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/pull/{}",
            self.repository, self.number
        )
    }
}

fn author_matches_user(author: &str, current_user: &str) -> bool {
    !current_user.is_empty() && author.eq_ignore_ascii_case(current_user)
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

    fn test_comment(author: &str, updated_at: DateTime<Utc>, is_review_comment: bool) -> PrComment {
        PrComment {
            id: format!("{author}-{}", updated_at.timestamp()),
            repository: "owner/repo".to_string(),
            pr_number: 42,
            author: author.to_string(),
            body: String::new(),
            created_at: updated_at,
            updated_at,
            is_review_comment,
            review_state: is_review_comment.then(|| "COMMENTED".to_string()),
        }
    }

    #[test]
    fn meaningful_changes_ignores_new_commit_for_my_pr() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.ack_display_changes(&author()).is_empty());
    }

    #[test]
    fn meaningful_changes_keeps_new_commit_for_tracked_pr() {
        let mut pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);
        pr.requested_reviewers = vec![not_author()];

        assert_eq!(
            pr.ack_display_changes(&not_author()),
            vec![ChangeKind::NewCommit]
        );
    }

    #[test]
    fn meaningful_changes_ignores_new_commit_for_unrelated_tracked_pr() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.ack_display_changes("reviewer").is_empty());
    }

    #[test]
    fn ack_display_changes_keeps_new_pr_for_author() {
        let pr = build_pull_request(&[]);

        assert_eq!(
            pr.ack_display_changes(&author()),
            vec![ChangeKind::NewPullRequest]
        );
    }

    #[test]
    fn meaningful_changes_keeps_new_commit_for_previously_reviewed_tracked_pr() {
        let mut pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);
        pr.user_has_reviewed = true;

        assert_eq!(
            pr.ack_display_changes("reviewer"),
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
        let ack = timestamp(2);
        let comment_at = timestamp(3);
        let mut pr = build_pull_request(&[]);
        pr.last_acknowledged_at = Some(ack);
        pr.last_comment_at = comment_at;
        pr.updated_at = comment_at;
        pr.comments = vec![test_comment(&not_author(), comment_at, false)];

        assert!(!pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_stays_true_for_my_own_comment() {
        let ack = timestamp(2);
        let comment_at = timestamp(3);
        let mut pr = build_pull_request(&[]);
        pr.last_acknowledged_at = Some(ack);
        pr.last_comment_at = comment_at;
        pr.updated_at = comment_at;
        pr.comments = vec![test_comment(&author(), comment_at, false)];

        assert!(pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_resets_for_other_users_comment() {
        let ack = timestamp(2);
        let comment_at = timestamp(3);
        let mut pr = build_pull_request(&[]);
        pr.last_acknowledged_at = Some(ack);
        pr.last_comment_at = comment_at;
        pr.updated_at = comment_at;
        pr.comments = vec![test_comment(&not_author(), comment_at, false)];

        assert!(!pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_stays_true_for_my_own_review() {
        let ack = timestamp(2);
        let review_at = timestamp(3);
        let mut pr = build_pull_request(&[]);
        pr.last_acknowledged_at = Some(ack);
        pr.last_comment_at = review_at;
        pr.last_review_status_update_at = review_at;
        pr.updated_at = review_at;
        pr.comments = vec![test_comment(&author(), review_at, true)];

        assert!(pr.is_acknowledged_for_user(&author()));
    }

    #[test]
    fn is_acknowledged_for_user_resets_for_tracked_pr_new_commit() {
        let mut pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);
        pr.requested_reviewers = vec![not_author()];

        assert!(!pr.is_acknowledged_for_user(&not_author()));
    }

    #[test]
    fn is_acknowledged_for_user_stays_true_for_tracked_pr_new_commit_when_unrelated() {
        let pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::Commit]);

        assert!(pr.is_acknowledged_for_user("reviewer"));
    }

    #[test]
    fn is_acknowledged_for_user_stays_true_for_pending_ci_change() {
        let mut pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::CiStatus]);
        pr.ci_status = CiStatus::Pending;

        assert!(pr.is_acknowledged_for_user(&not_author()));
    }

    #[test]
    fn meaningful_changes_keeps_non_pending_ci_change() {
        let mut pr = build_pull_request(&[TestPrEvent::Ack, TestPrEvent::CiStatus]);
        pr.ci_status = CiStatus::Failure;

        assert_eq!(
            pr.ack_display_changes(&not_author()),
            vec![ChangeKind::NewCistatus]
        );
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
    fn repository_name_returns_repo_without_owner_prefix() {
        let pr = build_pull_request(&[]);

        assert_eq!(pr.repository_name(), "repo");
    }

    #[test]
    fn repository_name_returns_original_value_when_no_owner_prefix_exists() {
        let mut pr = build_pull_request(&[]);
        pr.repository = "repo".to_string();

        assert_eq!(pr.repository_name(), "repo");
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
