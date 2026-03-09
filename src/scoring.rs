use crate::models::{ApprovalStatus, CiStatus, PrPerspective, PullRequest};

/// Computes an importance score for a PR relative to the given user.
///
/// Higher scores indicate PRs that need the user's attention more urgently.
/// The score uses additive points with the ownership axis (~1000 points)
/// dominating, and CI/draft/approval as secondary adjusters (~50-200 points).
pub fn importance_score(pr: &PullRequest, username: &str) -> i64 {
    match pr.perspective(username) {
        PrPerspective::MyPr => importance_score_for_my_pr(pr),
        PrPerspective::TrackedPr => importance_score_for_tracked_pr(pr, username),
    }
}

fn importance_score_for_my_pr(pr: &PullRequest) -> i64 {
    let mut score: i64 = 1000;

    if pr.draft {
        score -= 100;
    }

    match pr.ci_status {
        CiStatus::Failure => score += 100,
        CiStatus::Success => score += 50,
        CiStatus::Pending => {}
    }

    match pr.approval_status {
        ApprovalStatus::ChangesRequested => score += 100,
        ApprovalStatus::Approved => score += 50,
        ApprovalStatus::None => {}
    }

    score
}

fn importance_score_for_tracked_pr(pr: &PullRequest, username: &str) -> i64 {
    let mut score: i64 = 0;
    let is_requested_reviewer = !username.is_empty()
        && pr
            .requested_reviewers
            .iter()
            .any(|reviewer| reviewer.eq_ignore_ascii_case(username));

    if is_requested_reviewer {
        score += 500;
    }

    if pr.user_has_reviewed {
        score += 100;
    }

    if pr.draft {
        score -= 200;
    }

    match pr.ci_status {
        CiStatus::Failure => score -= 50,
        CiStatus::Success => score += 50,
        CiStatus::Pending => {}
    }

    match pr.approval_status {
        ApprovalStatus::ChangesRequested => {}
        ApprovalStatus::Approved => score -= 100,
        ApprovalStatus::None => {}
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::DateTime;

    fn test_pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: String::new(),
            repository: "org/repo".to_string(),
            author: String::new(),
            head_sha: String::new(),
            draft: false,
            created_at: DateTime::UNIX_EPOCH,
            updated_at: DateTime::UNIX_EPOCH,
            ci_status: CiStatus::Pending,
            last_comment_at: DateTime::UNIX_EPOCH,
            last_commit_at: DateTime::UNIX_EPOCH,
            last_ci_status_update_at: DateTime::UNIX_EPOCH,
            approval_status: ApprovalStatus::None,
            last_review_status_update_at: DateTime::UNIX_EPOCH,
            last_acknowledged_at: None,
            requested_reviewers: Vec::new(),
            user_has_reviewed: false,
            comments: Vec::new(),
        }
    }

    // ── Ownership axis ──────────────────────────────────────────────

    #[test]
    fn author_scores_higher_than_non_author() {
        let mut pr = test_pr();
        pr.author = "alice".to_string();

        let author_score = importance_score(&pr, "alice");

        let mut pr2 = test_pr();
        pr2.author = "bob".to_string();

        let non_author_score = importance_score(&pr2, "alice");

        assert!(author_score > non_author_score);
    }

    #[test]
    fn requested_reviewer_scores_higher_than_unrelated() {
        let mut pr = test_pr();
        pr.author = "bob".to_string();
        pr.requested_reviewers = vec!["alice".to_string()];

        let reviewer_score = importance_score(&pr, "alice");

        let mut pr2 = test_pr();
        pr2.author = "bob".to_string();

        let unrelated_score = importance_score(&pr2, "alice");

        assert!(reviewer_score > unrelated_score);
    }

    #[test]
    fn author_scores_higher_than_requested_reviewer() {
        let mut pr_author = test_pr();
        pr_author.author = "alice".to_string();

        let author_score = importance_score(&pr_author, "alice");

        let mut pr_reviewer = test_pr();
        pr_reviewer.author = "bob".to_string();
        pr_reviewer.requested_reviewers = vec!["alice".to_string()];

        let reviewer_score = importance_score(&pr_reviewer, "alice");

        assert!(author_score > reviewer_score);
    }

    // ── Case insensitivity ──────────────────────────────────────────

    #[test]
    fn case_insensitive_author_match() {
        let mut pr = test_pr();
        pr.author = "Alice".to_string();

        let score = importance_score(&pr, "alice");

        // Should get the author bonus (1000)
        let baseline = importance_score(&test_pr(), "alice");
        assert!(score > baseline);
        assert_eq!(score, 1000);
    }

    #[test]
    fn case_insensitive_reviewer_match() {
        let mut pr = test_pr();
        pr.author = "bob".to_string();
        pr.requested_reviewers = vec!["Alice".to_string()];

        let score = importance_score(&pr, "alice");

        let mut pr2 = test_pr();
        pr2.author = "bob".to_string();

        let baseline = importance_score(&pr2, "alice");
        assert!(score > baseline);
        assert_eq!(score, 500);
    }

    // ── Draft penalty ───────────────────────────────────────────────

    #[test]
    fn draft_reduces_non_author_score() {
        let mut non_draft = test_pr();
        non_draft.author = "bob".to_string();

        let mut draft = test_pr();
        draft.author = "bob".to_string();
        draft.draft = true;

        let non_draft_score = importance_score(&non_draft, "alice");
        let draft_score = importance_score(&draft, "alice");

        assert!(non_draft_score > draft_score);
    }

    #[test]
    fn draft_reduces_author_score_less() {
        let mut author_non_draft = test_pr();
        author_non_draft.author = "alice".to_string();

        let mut author_draft = test_pr();
        author_draft.author = "alice".to_string();
        author_draft.draft = true;

        let author_penalty =
            importance_score(&author_non_draft, "alice") - importance_score(&author_draft, "alice");

        let mut other_non_draft = test_pr();
        other_non_draft.author = "bob".to_string();

        let mut other_draft = test_pr();
        other_draft.author = "bob".to_string();
        other_draft.draft = true;

        let other_penalty =
            importance_score(&other_non_draft, "alice") - importance_score(&other_draft, "alice");

        // Author penalty is -100, non-author penalty is -200
        assert!(author_penalty < other_penalty);
        assert_eq!(author_penalty, 100);
        assert_eq!(other_penalty, 200);
    }

    #[test]
    fn draft_author_scores_higher_than_draft_non_author() {
        let mut author_draft = test_pr();
        author_draft.author = "alice".to_string();
        author_draft.draft = true;

        let mut other_draft = test_pr();
        other_draft.author = "bob".to_string();
        other_draft.draft = true;

        assert!(importance_score(&author_draft, "alice") > importance_score(&other_draft, "alice"));
    }

    // ── CI status for author ────────────────────────────────────────

    #[test]
    fn author_ci_failure_scores_highest() {
        let mut failure = test_pr();
        failure.author = "alice".to_string();
        failure.ci_status = CiStatus::Failure;

        let mut success = test_pr();
        success.author = "alice".to_string();
        success.ci_status = CiStatus::Success;

        let mut pending = test_pr();
        pending.author = "alice".to_string();
        pending.ci_status = CiStatus::Pending;

        let f = importance_score(&failure, "alice");
        let s = importance_score(&success, "alice");
        let p = importance_score(&pending, "alice");

        assert!(f > s);
        assert!(s > p);
    }

    #[test]
    fn author_ci_success_scores_above_pending() {
        let mut success = test_pr();
        success.author = "alice".to_string();
        success.ci_status = CiStatus::Success;

        let mut pending = test_pr();
        pending.author = "alice".to_string();
        pending.ci_status = CiStatus::Pending;

        assert!(importance_score(&success, "alice") > importance_score(&pending, "alice"));
    }

    // ── CI status for non-author ────────────────────────────────────

    #[test]
    fn non_author_ci_success_scores_highest() {
        let mut success = test_pr();
        success.author = "bob".to_string();
        success.ci_status = CiStatus::Success;

        let mut pending = test_pr();
        pending.author = "bob".to_string();
        pending.ci_status = CiStatus::Pending;

        let mut failure = test_pr();
        failure.author = "bob".to_string();
        failure.ci_status = CiStatus::Failure;

        let s = importance_score(&success, "alice");
        let p = importance_score(&pending, "alice");
        let f = importance_score(&failure, "alice");

        assert!(s > p);
        assert!(p > f);
    }

    #[test]
    fn non_author_ci_failure_scores_lowest() {
        let mut failure = test_pr();
        failure.author = "bob".to_string();
        failure.ci_status = CiStatus::Failure;

        let mut pending = test_pr();
        pending.author = "bob".to_string();
        pending.ci_status = CiStatus::Pending;

        assert!(importance_score(&failure, "alice") < importance_score(&pending, "alice"));
    }

    // ── Approval status for author ──────────────────────────────────

    #[test]
    fn author_changes_requested_boosts_score() {
        let mut changes = test_pr();
        changes.author = "alice".to_string();
        changes.approval_status = ApprovalStatus::ChangesRequested;

        let mut none = test_pr();
        none.author = "alice".to_string();

        assert!(importance_score(&changes, "alice") > importance_score(&none, "alice"));
    }

    #[test]
    fn author_approved_boosts_score() {
        let mut approved = test_pr();
        approved.author = "alice".to_string();
        approved.approval_status = ApprovalStatus::Approved;

        let mut none = test_pr();
        none.author = "alice".to_string();

        assert!(importance_score(&approved, "alice") > importance_score(&none, "alice"));
    }

    #[test]
    fn author_changes_requested_scores_above_approved() {
        let mut changes = test_pr();
        changes.author = "alice".to_string();
        changes.approval_status = ApprovalStatus::ChangesRequested;

        let mut approved = test_pr();
        approved.author = "alice".to_string();
        approved.approval_status = ApprovalStatus::Approved;

        assert!(importance_score(&changes, "alice") > importance_score(&approved, "alice"));
    }

    // ── Approval status for non-author ──────────────────────────────

    #[test]
    fn non_author_approved_reduces_score() {
        let mut approved = test_pr();
        approved.author = "bob".to_string();
        approved.approval_status = ApprovalStatus::Approved;

        let mut none = test_pr();
        none.author = "bob".to_string();

        assert!(importance_score(&approved, "alice") < importance_score(&none, "alice"));
    }

    #[test]
    fn non_author_changes_requested_neutral() {
        let mut changes = test_pr();
        changes.author = "bob".to_string();
        changes.approval_status = ApprovalStatus::ChangesRequested;

        let mut none = test_pr();
        none.author = "bob".to_string();

        assert_eq!(
            importance_score(&changes, "alice"),
            importance_score(&none, "alice")
        );
    }

    // ── Combined scenarios ──────────────────────────────────────────

    #[test]
    fn worst_case_score() {
        let mut pr = test_pr();
        pr.author = "bob".to_string();
        pr.draft = true;
        pr.ci_status = CiStatus::Failure;
        pr.approval_status = ApprovalStatus::Approved;

        // non-author: 0, draft: -200, CI failure: -50, approved: -100 = -350
        assert_eq!(importance_score(&pr, "alice"), -350);
    }

    #[test]
    fn best_case_author_score() {
        let mut pr = test_pr();
        pr.author = "alice".to_string();
        pr.draft = false;
        pr.ci_status = CiStatus::Failure;
        pr.approval_status = ApprovalStatus::ChangesRequested;

        // author: 1000, CI failure: +100, changes requested: +100 = 1200
        assert_eq!(importance_score(&pr, "alice"), 1200);
    }

    #[test]
    fn best_case_reviewer_score() {
        let mut pr = test_pr();
        pr.author = "bob".to_string();
        pr.requested_reviewers = vec!["alice".to_string()];
        pr.draft = false;
        pr.ci_status = CiStatus::Success;
        pr.approval_status = ApprovalStatus::None;

        // reviewer: 500, CI success: +50 = 550
        assert_eq!(importance_score(&pr, "alice"), 550);
    }

    #[test]
    fn previously_reviewed_tracked_pr_scores_above_unrelated() {
        let mut reviewed = test_pr();
        reviewed.author = "bob".to_string();
        reviewed.user_has_reviewed = true;

        let mut unrelated = test_pr();
        unrelated.author = "bob".to_string();

        assert!(importance_score(&reviewed, "alice") > importance_score(&unrelated, "alice"));
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn empty_username_no_bonuses() {
        let mut pr = test_pr();
        pr.author = "alice".to_string();
        pr.requested_reviewers = vec!["alice".to_string()];

        // Empty username should not match anything
        assert_eq!(importance_score(&pr, ""), 0);
    }

    #[test]
    fn same_dimensions_same_score() {
        let mut pr1 = test_pr();
        pr1.author = "alice".to_string();
        pr1.ci_status = CiStatus::Success;
        pr1.number = 10;
        pr1.title = "First PR".to_string();

        let mut pr2 = test_pr();
        pr2.author = "alice".to_string();
        pr2.ci_status = CiStatus::Success;
        pr2.number = 20;
        pr2.title = "Second PR".to_string();

        assert_eq!(
            importance_score(&pr1, "alice"),
            importance_score(&pr2, "alice")
        );
    }

    #[test]
    fn author_who_is_also_requested_reviewer_gets_author_bonus() {
        let mut pr = test_pr();
        pr.author = "alice".to_string();
        pr.requested_reviewers = vec!["alice".to_string()];

        // Should get author bonus (1000), NOT author + reviewer (1500)
        assert_eq!(importance_score(&pr, "alice"), 1000);
    }
}
