use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::models::{ApprovalStatus, CiStatus, PullRequest};

/// Style for CI status badges.
pub fn ci_style(status: CiStatus) -> Style {
    match status {
        CiStatus::Pending => Style::default().fg(Color::LightYellow),
        CiStatus::Success => Style::default().fg(Color::LightGreen),
        CiStatus::Failure => Style::default().fg(Color::LightRed),
    }
}

/// Label text for CI status.
pub fn ci_label(status: CiStatus) -> &'static str {
    match status {
        CiStatus::Pending => "pending",
        CiStatus::Success => "success",
        CiStatus::Failure => "failure",
    }
}

/// Badge showing approval status of a PR.
pub fn approval_badge(pr: &PullRequest) -> Span<'static> {
    match pr.approval_status {
        ApprovalStatus::None => Span::styled("  no reviews", Style::default().fg(Color::LightGray)),
        ApprovalStatus::Approved => {
            Span::styled("  approved", Style::default().fg(Color::LightGreen))
        }
        ApprovalStatus::ChangesRequested => {
            Span::styled("  changes requested", Style::default().fg(Color::LightRed))
        }
    }
}

/// Badge showing if the user is involved in a PR.
pub fn involved_badge<'a>(pr: &PullRequest, username: &str) -> Span<'a> {
    if pr.user_is_involved(username) {
        Span::styled(
            "  involved",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    }
}

/// Badge showing review status for the current user.
pub fn review_badge<'a>(pr: &PullRequest, username: &str) -> Span<'a> {
    if username.is_empty() || pr.author.eq_ignore_ascii_case(username) {
        return Span::raw("");
    }

    if pr.user_is_involved(username) {
        Span::styled(
            "  review requested",
            Style::default().fg(Color::LightYellow),
        )
    } else if pr.user_has_reviewed {
        Span::styled("  reviewed", Style::default().fg(Color::LightGreen))
    } else {
        Span::raw("")
    }
}

/// Spinner frame character for the given tick.
pub fn spinner_frame(tick: usize) -> char {
    const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
    FRAMES[tick % FRAMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::DateTime;

    fn test_pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: "Test PR".to_string(),
            repository: "owner/repo".to_string(),
            author: "alice".to_string(),
            head_sha: "abc123".to_string(),
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

    // ── approval_badge tests ─────────────────────────────────────────

    #[test]
    fn approval_badge_none_shows_no_reviews() {
        let mut pr = test_pr();
        pr.approval_status = ApprovalStatus::None;

        let badge = approval_badge(&pr);

        assert_eq!(badge.content, "  no reviews");
    }

    #[test]
    fn approval_badge_approved_shows_approved() {
        let mut pr = test_pr();
        pr.approval_status = ApprovalStatus::Approved;

        let badge = approval_badge(&pr);

        assert_eq!(badge.content, "  approved");
    }

    #[test]
    fn approval_badge_changes_requested_shows_changes() {
        let mut pr = test_pr();
        pr.approval_status = ApprovalStatus::ChangesRequested;

        let badge = approval_badge(&pr);

        assert_eq!(badge.content, "  changes requested");
    }

    // ── involved_badge tests ───────────────────────────────────────

    #[test]
    fn involved_badge_shows_involved_when_user_is_involved() {
        let mut pr = test_pr();
        pr.requested_reviewers = vec!["bob".to_string()];

        let badge = involved_badge(&pr, "bob");

        assert_eq!(badge.content, "  involved");
    }

    #[test]
    fn involved_badge_empty_when_user_not_involved() {
        let pr = test_pr();

        let badge = involved_badge(&pr, "bob");

        assert_eq!(badge.content, "");
    }

    #[test]
    fn involved_badge_empty_for_author_only() {
        let pr = test_pr();

        let badge = involved_badge(&pr, "alice");

        assert_eq!(badge.content, "  involved");
    }

    // ── review_badge tests ─────────────────────────────────────────

    #[test]
    fn review_badge_empty_when_empty_username() {
        let pr = test_pr();

        let badge = review_badge(&pr, "");

        assert_eq!(badge.content, "");
    }

    #[test]
    fn review_badge_empty_when_user_is_author() {
        let pr = test_pr();

        let badge = review_badge(&pr, "alice");

        assert_eq!(badge.content, "");
    }

    #[test]
    fn review_badge_shows_review_requested_when_involved() {
        let mut pr = test_pr();
        pr.requested_reviewers = vec!["bob".to_string()];

        let badge = review_badge(&pr, "bob");

        assert_eq!(badge.content, "  review requested");
    }

    #[test]
    fn review_badge_shows_reviewed_when_user_has_reviewed() {
        let mut pr = test_pr();
        pr.user_has_reviewed = true;

        let badge = review_badge(&pr, "bob");

        assert_eq!(badge.content, "  reviewed");
    }

    #[test]
    fn review_badge_empty_when_not_involved_and_not_reviewed() {
        let pr = test_pr();

        let badge = review_badge(&pr, "bob");

        assert_eq!(badge.content, "");
    }

    // ── ci_style tests ───────────────────────────────────────────────

    #[test]
    fn ci_style_pending_is_light_yellow() {
        let style = ci_style(CiStatus::Pending);
        assert_eq!(style.fg, Some(Color::LightYellow));
    }

    #[test]
    fn ci_style_success_is_light_green() {
        let style = ci_style(CiStatus::Success);
        assert_eq!(style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn ci_style_failure_is_light_red() {
        let style = ci_style(CiStatus::Failure);
        assert_eq!(style.fg, Some(Color::LightRed));
    }

    // ── ci_label tests ─────────────────────────────────────────────

    #[test]
    fn ci_label_pending() {
        assert_eq!(ci_label(CiStatus::Pending), "pending");
    }

    #[test]
    fn ci_label_success() {
        assert_eq!(ci_label(CiStatus::Success), "success");
    }

    #[test]
    fn ci_label_failure() {
        assert_eq!(ci_label(CiStatus::Failure), "failure");
    }

    // ── spinner_frame tests ─────────────────────────────────────────

    #[test]
    fn spinner_frame_cycles_through_frames() {
        assert_eq!(spinner_frame(0), '|');
        assert_eq!(spinner_frame(1), '/');
        assert_eq!(spinner_frame(2), '-');
        assert_eq!(spinner_frame(3), '\\');
    }

    #[test]
    fn spinner_frame_cycles_back_to_start() {
        assert_eq!(spinner_frame(4), '|');
        assert_eq!(spinner_frame(5), '/');
        assert_eq!(spinner_frame(8), '|');
        assert_eq!(spinner_frame(12), '|');
    }

    #[test]
    fn spinner_frame_handles_large_ticks() {
        assert_eq!(spinner_frame(100), '|');
        assert_eq!(spinner_frame(101), '/');
    }
}
