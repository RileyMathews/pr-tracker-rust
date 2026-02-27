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

        let (ci_status_changed, has_relevant_changes) =
            pull_request_has_relevant_changes(existing_pr, incoming_pr);
        if !has_relevant_changes {
            continue;
        }

        let mut updated = incoming_pr.clone();
        apply_sync_metadata(existing_pr, &mut updated, ci_status_changed, now);
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
) -> (bool, bool) {
    let ci_status_changed = existing_pr.ci_status != incoming_pr.ci_status;
    let last_comment_changed = existing_pr.last_comment_at != incoming_pr.last_comment_at;
    let last_commit_changed = existing_pr.last_commit_at != incoming_pr.last_commit_at;

    (
        ci_status_changed,
        ci_status_changed || last_comment_changed || last_commit_changed,
    )
}

fn apply_sync_metadata(
    existing_pr: &PullRequest,
    incoming_pr: &mut PullRequest,
    ci_status_changed: bool,
    now: DateTime<Utc>,
) {
    incoming_pr.last_acknowledged_at = existing_pr.last_acknowledged_at;
    incoming_pr.last_ci_status_update_at = if ci_status_changed {
        now
    } else {
        existing_pr.last_ci_status_update_at
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::process_pull_request_sync_results;
    use crate::models::{CiStatus, PullRequest};

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
            draft: false,
            created_at: DateTime::UNIX_EPOCH,
            updated_at: DateTime::UNIX_EPOCH,
            ci_status: CiStatus::Pending,
            last_comment_at: DateTime::UNIX_EPOCH,
            last_commit_at: DateTime::UNIX_EPOCH,
            last_ci_status_update_at: DateTime::UNIX_EPOCH,
            last_acknowledged_at: None,
            requested_reviewers: Vec::new(),
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
            last_commit_at: later,
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
}
