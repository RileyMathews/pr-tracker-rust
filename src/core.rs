use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::models::PullRequest;

#[derive(Debug, Default)]
pub struct SyncDiff {
    pub new_prs: Vec<PullRequest>,
    pub updated_prs: Vec<PullRequest>,
    pub removed_prs: Vec<PullRequest>,
}

pub fn process_pull_request_sync_results(
    prs_from_database: &[PullRequest],
    prs_from_fresh_sync: &[PullRequest],
    now: DateTime<Utc>,
) -> SyncDiff {
    let db_by_key = index_pull_requests_by_key(prs_from_database);
    let mut seen = HashSet::with_capacity(prs_from_fresh_sync.len());

    let mut diff = SyncDiff::default();

    for incoming_pr in prs_from_fresh_sync {
        let key = pull_request_key(incoming_pr);
        seen.insert(key.clone());

        let Some(existing_pr) = db_by_key.get(&key) else {
            diff.new_prs.push(incoming_pr.clone());
            continue;
        };

        let (ci_status_changed, approval_status_changed, has_relevant_changes) =
            pull_request_has_relevant_changes(existing_pr, incoming_pr);
        if !has_relevant_changes {
            continue;
        }

        let mut updated = incoming_pr.clone();
        apply_sync_metadata(
            existing_pr,
            &mut updated,
            ci_status_changed,
            approval_status_changed,
            now,
        );
        diff.updated_prs.push(updated);
    }

    diff.removed_prs = collect_removed_pull_requests(&db_by_key, &seen);
    diff
}

fn pull_request_key(pr: &PullRequest) -> String {
    format!("{}#{}", pr.repository, pr.number)
}

fn index_pull_requests_by_key(prs: &[PullRequest]) -> HashMap<String, PullRequest> {
    prs.iter()
        .map(|pr| (pull_request_key(pr), pr.clone()))
        .collect()
}

fn pull_request_has_relevant_changes(
    existing_pr: &PullRequest,
    incoming_pr: &PullRequest,
) -> (bool, bool, bool) {
    let ci_status_changed = existing_pr.ci_status != incoming_pr.ci_status;
    let last_comment_changed = existing_pr.last_comment_at != incoming_pr.last_comment_at;
    let head_sha_changed = existing_pr.head_sha != incoming_pr.head_sha;
    let approval_status_changed = existing_pr.approval_status != incoming_pr.approval_status;
    let review_fields_changed = existing_pr.user_has_reviewed != incoming_pr.user_has_reviewed
        || existing_pr.requested_reviewers != incoming_pr.requested_reviewers;

    (
        ci_status_changed,
        approval_status_changed,
        ci_status_changed
            || last_comment_changed
            || head_sha_changed
            || approval_status_changed
            || review_fields_changed,
    )
}

fn apply_sync_metadata(
    existing_pr: &PullRequest,
    incoming_pr: &mut PullRequest,
    ci_status_changed: bool,
    approval_status_changed: bool,
    now: DateTime<Utc>,
) {
    incoming_pr.last_acknowledged_at = existing_pr.last_acknowledged_at;
    incoming_pr.last_commit_at = if existing_pr.head_sha != incoming_pr.head_sha {
        now
    } else {
        existing_pr.last_commit_at
    };
    incoming_pr.last_ci_status_update_at = if ci_status_changed {
        now
    } else {
        existing_pr.last_ci_status_update_at
    };
    incoming_pr.last_review_status_update_at = if approval_status_changed {
        incoming_pr.last_review_status_update_at
    } else {
        existing_pr.last_review_status_update_at
    };
}

fn collect_removed_pull_requests(
    db_by_key: &HashMap<String, PullRequest>,
    seen: &HashSet<String>,
) -> Vec<PullRequest> {
    db_by_key
        .iter()
        .filter_map(|(key, pr)| {
            if seen.contains(key) {
                None
            } else {
                Some(pr.clone())
            }
        })
        .collect()
}

/// Result of classifying team members into tracked and untracked groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedMembers {
    pub already_tracked: Vec<String>,
    pub candidates: Vec<String>,
}

/// Classify team members into already-tracked and new candidates.
///
/// Filters out the current user, then splits the remaining members into those
/// already being tracked and those who are candidates to be added. Both lists
/// are returned sorted alphabetically.
pub fn classify_team_members(
    all_members: Vec<String>,
    current_user: &str,
    tracked_authors: &[String],
) -> ClassifiedMembers {
    let current_lower = current_user.to_lowercase();
    let tracked_lower: HashSet<String> = tracked_authors.iter().map(|s| s.to_lowercase()).collect();

    let mut already_tracked = Vec::new();
    let mut candidates = Vec::new();

    for login in all_members {
        let lower = login.to_lowercase();
        if lower == current_lower {
            continue;
        }
        if tracked_lower.contains(&lower) {
            already_tracked.push(login);
        } else {
            candidates.push(login);
        }
    }

    already_tracked.sort();
    candidates.sort();

    ClassifiedMembers {
        already_tracked,
        candidates,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};

    fn dt(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("valid datetime")
    }

    fn empty_pr(repo: &str, number: i64) -> PullRequest {
        PullRequest {
            number,
            title: String::new(),
            repository: repo.to_string(),
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

    #[test]
    fn classifies_new_pr() {
        let pr = empty_pr("acme/repo", 1);
        let result = process_pull_request_sync_results(&[], &[pr.clone()], Utc::now());

        assert_eq!(result.new_prs, vec![pr]);
        assert!(result.updated_prs.is_empty());
        assert!(result.removed_prs.is_empty());
    }

    #[test]
    fn classifies_updated_pr() {
        let base = dt(2025, 1, 1, 0);
        let db_pr = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            ci_status: CiStatus::Success,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr.clone()], Utc::now());
        assert!(result.new_prs.is_empty());
        assert_eq!(result.updated_prs.len(), 1);
        assert_eq!(result.updated_prs[0].number, fresh_pr.number);
        assert!(result.removed_prs.is_empty());
    }

    #[test]
    fn classifies_removed_pr() {
        let pr = empty_pr("acme/repo", 1);
        let result = process_pull_request_sync_results(&[pr.clone()], &[], Utc::now());
        assert!(result.new_prs.is_empty());
        assert!(result.updated_prs.is_empty());
        assert_eq!(result.removed_prs, vec![pr]);
    }

    #[test]
    fn mixed_classification() {
        let base = dt(2025, 1, 1, 0);
        let later = dt(2025, 1, 1, 1);

        let db1 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 1)
        };
        let db2 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            head_sha: "sha-1".to_string(),
            ..empty_pr("org/repo", 2)
        };
        let db3 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 3)
        };
        let db4 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 4)
        };
        let db6 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 6)
        };

        let fresh1 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 1)
        };
        let fresh2 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: base,
            head_sha: "sha-2".to_string(),
            ..empty_pr("org/repo", 2)
        };
        let fresh3 = PullRequest {
            ci_status: CiStatus::Pending,
            last_comment_at: later,
            last_commit_at: base,
            ..empty_pr("org/repo", 3)
        };
        let fresh4 = PullRequest {
            ci_status: CiStatus::Failure,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 4)
        };
        let fresh5 = PullRequest {
            ci_status: CiStatus::Success,
            last_comment_at: base,
            last_commit_at: base,
            ..empty_pr("org/repo", 5)
        };

        let result = process_pull_request_sync_results(
            &[db1, db2, db3, db4, db6],
            &[fresh1, fresh2, fresh3, fresh4, fresh5],
            Utc::now(),
        );

        assert_eq!(result.new_prs.len(), 1);
        assert_eq!(result.new_prs[0].number, 5);

        assert_eq!(result.updated_prs.len(), 3);
        let updated_numbers: std::collections::HashSet<i64> =
            result.updated_prs.iter().map(|pr| pr.number).collect();
        assert!(updated_numbers.contains(&2));
        assert!(updated_numbers.contains(&3));
        assert!(updated_numbers.contains(&4));

        assert_eq!(result.removed_prs.len(), 1);
        assert_eq!(result.removed_prs[0].number, 6);
    }

    #[test]
    fn updates_last_commit_when_head_sha_changes() {
        let before = dt(2025, 1, 1, 0);
        let now = dt(2025, 1, 1, 2);

        let db_pr = PullRequest {
            head_sha: "old-sha".to_string(),
            last_commit_at: before,
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            head_sha: "new-sha".to_string(),
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], now);
        assert_eq!(result.updated_prs.len(), 1);
        assert_eq!(result.updated_prs[0].last_commit_at, now);
    }

    #[test]
    fn updates_review_status_from_api_timestamp() {
        let before = dt(2025, 1, 1, 0);
        let api_review_time = dt(2025, 1, 1, 1);
        let now = dt(2025, 1, 1, 2);

        let db_pr = PullRequest {
            approval_status: ApprovalStatus::None,
            last_review_status_update_at: before,
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            approval_status: ApprovalStatus::Approved,
            last_review_status_update_at: api_review_time,
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], now);
        assert_eq!(result.updated_prs.len(), 1);
        // Key assertion: the timestamp comes from the API (api_review_time), not from `now`
        assert_eq!(
            result.updated_prs[0].last_review_status_update_at,
            api_review_time
        );
        assert_eq!(
            result.updated_prs[0].approval_status,
            ApprovalStatus::Approved
        );
    }

    // -- classify_team_members tests --

    #[test]
    fn classify_excludes_current_user() {
        let members = vec!["Alice".to_string(), "Bob".to_string()];
        let result = classify_team_members(members, "alice", &[]);

        assert!(result.already_tracked.is_empty());
        assert_eq!(result.candidates, vec!["Bob"]);
    }

    #[test]
    fn classify_splits_tracked_and_candidates() {
        let members = vec!["Alice".to_string(), "Bob".to_string(), "Carol".to_string()];
        let tracked = vec!["bob".to_string()];
        let result = classify_team_members(members, "alice", &tracked);

        assert_eq!(result.already_tracked, vec!["Bob"]);
        assert_eq!(result.candidates, vec!["Carol"]);
    }

    #[test]
    fn classify_case_insensitive_tracking() {
        let members = vec!["BOB".to_string()];
        let tracked = vec!["bob".to_string()];
        let result = classify_team_members(members, "alice", &tracked);

        assert_eq!(result.already_tracked, vec!["BOB"]);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn classify_returns_sorted_results() {
        let members = vec![
            "Zara".to_string(),
            "Alice".to_string(),
            "Me".to_string(),
            "Bob".to_string(),
        ];
        let tracked = vec!["zara".to_string()];
        let result = classify_team_members(members, "me", &tracked);

        assert_eq!(result.already_tracked, vec!["Zara"]);
        assert_eq!(result.candidates, vec!["Alice", "Bob"]);
    }

    #[test]
    fn classify_empty_members_returns_empty() {
        let result = classify_team_members(vec![], "alice", &["bob".to_string()]);

        assert!(result.already_tracked.is_empty());
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn classify_all_tracked_returns_no_candidates() {
        let members = vec!["Bob".to_string(), "Carol".to_string()];
        let tracked = vec!["bob".to_string(), "carol".to_string()];
        let result = classify_team_members(members, "alice", &tracked);

        assert_eq!(result.already_tracked, vec!["Bob", "Carol"]);
        assert!(result.candidates.is_empty());
    }
}
