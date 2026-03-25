use std::collections::HashSet;

use crate::models::PullRequest;
use crate::scoring;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrOwnerFilter {
    Tracked,
    Mine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrStatusFilter {
    Active,
    Acknowledged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrListQuery {
    pub owner: PrOwnerFilter,
    pub status: PrStatusFilter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamAuthorBuckets {
    pub tracked: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDashboard {
    pub prs: Vec<PullRequest>,
    pub active_tracked: Vec<usize>,
    pub active_mine: Vec<usize>,
    pub acknowledged_tracked: Vec<usize>,
    pub acknowledged_mine: Vec<usize>,
}

impl PrDashboard {
    pub fn section(&self, owner: PrOwnerFilter, status: PrStatusFilter) -> &[usize] {
        match (owner, status) {
            (PrOwnerFilter::Tracked, PrStatusFilter::Active) => &self.active_tracked,
            (PrOwnerFilter::Mine, PrStatusFilter::Active) => &self.active_mine,
            (PrOwnerFilter::Tracked, PrStatusFilter::Acknowledged) => &self.acknowledged_tracked,
            (PrOwnerFilter::Mine, PrStatusFilter::Acknowledged) => &self.acknowledged_mine,
        }
    }
}

pub fn build_pr_dashboard(prs: Vec<PullRequest>, username: &str) -> PrDashboard {
    let active_tracked = filtered_pr_indices(
        &prs,
        username,
        PrListQuery {
            owner: PrOwnerFilter::Tracked,
            status: PrStatusFilter::Active,
        },
    );
    let active_mine = filtered_pr_indices(
        &prs,
        username,
        PrListQuery {
            owner: PrOwnerFilter::Mine,
            status: PrStatusFilter::Active,
        },
    );
    let acknowledged_tracked = filtered_pr_indices(
        &prs,
        username,
        PrListQuery {
            owner: PrOwnerFilter::Tracked,
            status: PrStatusFilter::Acknowledged,
        },
    );
    let acknowledged_mine = filtered_pr_indices(
        &prs,
        username,
        PrListQuery {
            owner: PrOwnerFilter::Mine,
            status: PrStatusFilter::Acknowledged,
        },
    );

    PrDashboard {
        prs,
        active_tracked,
        active_mine,
        acknowledged_tracked,
        acknowledged_mine,
    }
}

pub fn filtered_pr_indices(prs: &[PullRequest], username: &str, query: PrListQuery) -> Vec<usize> {
    let mut indices: Vec<usize> = prs
        .iter()
        .enumerate()
        .filter_map(|(index, pr)| matches_query(pr, username, query).then_some(index))
        .collect();

    indices.sort_by(|&a, &b| {
        let score_a = list_attention_score(&prs[a], username);
        let score_b = list_attention_score(&prs[b], username);
        let pr_a = &prs[a];
        let pr_b = &prs[b];
        score_b
            .cmp(&score_a)
            .then(pr_b.updated_at.cmp(&pr_a.updated_at))
            .then(pr_a.repository.cmp(&pr_b.repository))
            .then(pr_a.number.cmp(&pr_b.number))
    });

    indices
}

pub fn selected_pr_index(indices: &[usize], cursor: usize) -> Option<usize> {
    indices.get(cursor).copied()
}

pub fn partition_team_authors(
    team_members: Vec<String>,
    tracked_authors: &[String],
    current_user: &str,
) -> TeamAuthorBuckets {
    let tracked_set: HashSet<String> = tracked_authors.iter().map(|s| s.to_lowercase()).collect();
    let current_user = current_user.to_lowercase();
    let mut seen = HashSet::new();

    let mut tracked = Vec::new();
    let mut untracked = Vec::new();

    for login in team_members {
        let lower = login.to_lowercase();
        if lower == current_user || !seen.insert(lower.clone()) {
            continue;
        }

        if tracked_set.contains(&lower) {
            tracked.push(login);
        } else {
            untracked.push(login);
        }
    }

    tracked.sort();
    untracked.sort();

    TeamAuthorBuckets { tracked, untracked }
}

fn matches_query(pr: &PullRequest, username: &str, query: PrListQuery) -> bool {
    let matches_status = match query.status {
        PrStatusFilter::Active => !pr.is_acknowledged_for_user(username),
        PrStatusFilter::Acknowledged => pr.is_acknowledged_for_user(username),
    };

    let matches_owner = match query.owner {
        PrOwnerFilter::Tracked => !pr.is_mine(username),
        PrOwnerFilter::Mine => pr.is_mine(username),
    };

    matches_status && matches_owner
}

fn list_attention_score(pr: &PullRequest, username: &str) -> i64 {
    let mut score = scoring::importance_score(pr, username);
    if pr.user_is_involved(username) {
        score += 100;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::{DateTime, TimeZone, Utc};

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

    fn pr_with_author(number: i64, author: &str) -> PullRequest {
        let mut pr = test_pr();
        pr.number = number;
        pr.author = author.to_string();
        pr
    }

    fn pr_with_ack(number: i64, author: &str, ack: bool) -> PullRequest {
        let mut pr = pr_with_author(number, author);
        if ack {
            pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        }
        pr
    }

    #[test]
    fn filtered_pr_indices_exclude_my_prs_for_tracked_query() {
        let prs = vec![pr_with_author(1, "alice"), pr_with_author(2, "bob")];

        assert_eq!(
            filtered_pr_indices(
                &prs,
                "alice",
                PrListQuery {
                    owner: PrOwnerFilter::Tracked,
                    status: PrStatusFilter::Active,
                },
            ),
            vec![1]
        );
    }

    #[test]
    fn filtered_pr_indices_include_only_my_prs_for_mine_query() {
        let prs = vec![pr_with_author(1, "alice"), pr_with_author(2, "bob")];

        assert_eq!(
            filtered_pr_indices(
                &prs,
                "alice",
                PrListQuery {
                    owner: PrOwnerFilter::Mine,
                    status: PrStatusFilter::Active,
                },
            ),
            vec![0]
        );
    }

    #[test]
    fn filtered_pr_indices_filter_acknowledged_by_status() {
        let prs = vec![pr_with_ack(1, "bob", false), pr_with_ack(2, "bob", true)];

        assert_eq!(
            filtered_pr_indices(
                &prs,
                "alice",
                PrListQuery {
                    owner: PrOwnerFilter::Tracked,
                    status: PrStatusFilter::Acknowledged,
                },
            ),
            vec![1]
        );
    }

    #[test]
    fn filtered_pr_indices_sort_by_attention_then_updated() {
        let mut pr1 = pr_with_author(1, "bob");
        pr1.requested_reviewers = vec!["alice".to_string()];

        let mut pr2 = pr_with_author(2, "carol");
        pr2.updated_at = Utc.timestamp_opt(100, 0).unwrap();

        let prs = vec![pr2, pr1];

        assert_eq!(
            filtered_pr_indices(
                &prs,
                "alice",
                PrListQuery {
                    owner: PrOwnerFilter::Tracked,
                    status: PrStatusFilter::Active,
                },
            ),
            vec![1, 0]
        );
    }

    #[test]
    fn selected_pr_index_returns_none_when_out_of_range() {
        assert_eq!(selected_pr_index(&[2, 4], 3), None);
    }

    #[test]
    fn partition_team_authors_removes_self_duplicates_and_tracked() {
        let buckets = partition_team_authors(
            vec![
                "bob".to_string(),
                "alice".to_string(),
                "Bob".to_string(),
                "carol".to_string(),
            ],
            &["dave".to_string(), "carol".to_string()],
            "alice",
        );

        assert_eq!(buckets.tracked, vec!["carol".to_string()]);
        assert_eq!(buckets.untracked, vec!["bob".to_string()]);
    }

    #[test]
    fn build_pr_dashboard_populates_all_sections() {
        let mut mine_active = pr_with_author(1, "alice");
        mine_active.updated_at = Utc.timestamp_opt(100, 0).unwrap();

        let mut tracked_active = pr_with_author(2, "bob");
        tracked_active.requested_reviewers = vec!["alice".to_string()];

        let mut mine_ack = pr_with_author(3, "alice");
        mine_ack.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);

        let mut tracked_ack = pr_with_author(4, "carol");
        tracked_ack.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);

        let dashboard = build_pr_dashboard(
            vec![mine_active, tracked_active, mine_ack, tracked_ack],
            "alice",
        );

        assert_eq!(dashboard.active_tracked, vec![1]);
        assert_eq!(dashboard.active_mine, vec![0]);
        assert_eq!(dashboard.acknowledged_tracked, vec![3]);
        assert_eq!(dashboard.acknowledged_mine, vec![2]);
    }
}
