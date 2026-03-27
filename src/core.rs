use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::models::PullRequest;

#[derive(Debug, Default)]
pub struct SyncDiff {
    pub new_prs: Vec<PullRequest>,
    pub updated_prs: Vec<UpdatedPullRequest>,
    pub removed_prs: Vec<PullRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedPullRequest {
    pub pr: PullRequest,
    pub reasons: Vec<UpdateReason>,
    pub attention_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateReason {
    CiStatusChanged,
    LastCommentChanged,
    HeadShaChanged,
    ApprovalStatusChanged,
    RequestedReviewersChanged,
    UserReviewedChanged,
    DraftChanged,
    TitleChanged,
    UpdatedAtChanged,
}

impl UpdateReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::CiStatusChanged => "ci",
            Self::LastCommentChanged => "comment",
            Self::HeadShaChanged => "head_sha",
            Self::ApprovalStatusChanged => "approval",
            Self::RequestedReviewersChanged => "reviewers",
            Self::UserReviewedChanged => "user_reviewed",
            Self::DraftChanged => "draft",
            Self::TitleChanged => "title",
            Self::UpdatedAtChanged => "updated_at",
        }
    }
}

pub fn count_update_reasons(updated_prs: &[UpdatedPullRequest]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for updated in updated_prs {
        for reason in &updated.reasons {
            let code = reason.code().to_string();
            *counts.entry(code).or_insert(0) += 1;
        }
    }
    counts
}

pub fn partition_updated_pull_requests(
    updated_prs: Vec<UpdatedPullRequest>,
) -> (Vec<PullRequest>, Vec<PullRequest>) {
    let mut updated_data_prs = Vec::with_capacity(updated_prs.len());
    let mut updated_attention_prs = Vec::new();

    for updated in updated_prs {
        updated_data_prs.push(updated.pr.clone());
        if updated.attention_changed {
            updated_attention_prs.push(updated.pr);
        }
    }

    (updated_data_prs, updated_attention_prs)
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

        let update_analysis = analyze_pull_request_changes(existing_pr, incoming_pr);
        if !update_analysis.has_data_changes {
            continue;
        }

        let mut updated = incoming_pr.clone();
        apply_sync_metadata(
            existing_pr,
            &mut updated,
            update_analysis.ci_status_changed,
            update_analysis.approval_status_changed,
            now,
        );
        diff.updated_prs.push(UpdatedPullRequest {
            pr: updated,
            reasons: update_analysis.reasons,
            attention_changed: update_analysis.has_attention_changes,
        });
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

struct PullRequestUpdateAnalysis {
    ci_status_changed: bool,
    approval_status_changed: bool,
    has_attention_changes: bool,
    has_data_changes: bool,
    reasons: Vec<UpdateReason>,
}

fn analyze_pull_request_changes(
    existing_pr: &PullRequest,
    incoming_pr: &PullRequest,
) -> PullRequestUpdateAnalysis {
    let ci_status_changed = existing_pr.ci_status != incoming_pr.ci_status;
    let last_comment_changed = existing_pr.last_comment_at != incoming_pr.last_comment_at;
    let head_sha_changed = existing_pr.head_sha != incoming_pr.head_sha;
    let approval_status_changed = existing_pr.approval_status != incoming_pr.approval_status;
    let requested_reviewers_changed =
        existing_pr.requested_reviewers != incoming_pr.requested_reviewers;
    let user_reviewed_changed = existing_pr.user_has_reviewed != incoming_pr.user_has_reviewed;
    let draft_changed = existing_pr.draft != incoming_pr.draft;
    let title_changed = existing_pr.title != incoming_pr.title;
    let updated_at_changed = existing_pr.updated_at != incoming_pr.updated_at;

    let has_attention_changes = ci_status_changed
        || last_comment_changed
        || head_sha_changed
        || approval_status_changed
        || requested_reviewers_changed
        || user_reviewed_changed;

    let has_data_changes =
        has_attention_changes || draft_changed || title_changed || updated_at_changed;

    let mut reasons = Vec::new();
    if ci_status_changed {
        reasons.push(UpdateReason::CiStatusChanged);
    }
    if last_comment_changed {
        reasons.push(UpdateReason::LastCommentChanged);
    }
    if head_sha_changed {
        reasons.push(UpdateReason::HeadShaChanged);
    }
    if approval_status_changed {
        reasons.push(UpdateReason::ApprovalStatusChanged);
    }
    if requested_reviewers_changed {
        reasons.push(UpdateReason::RequestedReviewersChanged);
    }
    if user_reviewed_changed {
        reasons.push(UpdateReason::UserReviewedChanged);
    }
    if draft_changed {
        reasons.push(UpdateReason::DraftChanged);
    }
    if title_changed {
        reasons.push(UpdateReason::TitleChanged);
    }
    if updated_at_changed {
        reasons.push(UpdateReason::UpdatedAtChanged);
    }

    PullRequestUpdateAnalysis {
        ci_status_changed,
        approval_status_changed,
        has_attention_changes,
        has_data_changes,
        reasons,
    }
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::process_pull_request_sync_results;
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
        assert_eq!(result.updated_prs[0].pr.number, fresh_pr.number);
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
        let updated_numbers: std::collections::HashSet<i64> = result
            .updated_prs
            .iter()
            .map(|entry| entry.pr.number)
            .collect();
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
        assert_eq!(result.updated_prs[0].pr.last_commit_at, now);
    }

    #[test]
    fn updates_ci_status_even_when_updated_at_is_unchanged() {
        let before = dt(2025, 1, 1, 0);
        let now = dt(2025, 1, 1, 2);

        let db_pr = PullRequest {
            ci_status: CiStatus::Failure,
            updated_at: before,
            last_ci_status_update_at: before,
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            ci_status: CiStatus::Success,
            updated_at: before,
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], now);

        assert_eq!(result.updated_prs.len(), 1);
        assert_eq!(result.updated_prs[0].pr.ci_status, CiStatus::Success);
        assert_eq!(result.updated_prs[0].pr.last_ci_status_update_at, now);
        assert!(result.updated_prs[0]
            .reasons
            .contains(&super::UpdateReason::CiStatusChanged));
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
            result.updated_prs[0].pr.last_review_status_update_at,
            api_review_time
        );
        assert_eq!(
            result.updated_prs[0].pr.approval_status,
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn draft_only_change_is_data_update_without_attention_update() {
        let db_pr = PullRequest {
            draft: true,
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            draft: false,
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], Utc::now());

        assert_eq!(result.updated_prs.len(), 1);
        let update = &result.updated_prs[0];
        assert!(!update.attention_changed);
        assert!(update.reasons.contains(&super::UpdateReason::DraftChanged));
    }

    #[test]
    fn title_only_change_is_data_update_without_attention_update() {
        let db_pr = PullRequest {
            title: "old".to_string(),
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            title: "new".to_string(),
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], Utc::now());

        assert_eq!(result.updated_prs.len(), 1);
        let update = &result.updated_prs[0];
        assert!(!update.attention_changed);
        assert!(update.reasons.contains(&super::UpdateReason::TitleChanged));
    }

    #[test]
    fn update_reason_codes_are_aggregated() {
        let db_pr = PullRequest {
            draft: true,
            title: "old".to_string(),
            ..empty_pr("acme/repo", 1)
        };
        let fresh_pr = PullRequest {
            draft: false,
            title: "new".to_string(),
            ..empty_pr("acme/repo", 1)
        };

        let result = process_pull_request_sync_results(&[db_pr], &[fresh_pr], Utc::now());
        let counts = super::count_update_reasons(&result.updated_prs);

        assert_eq!(counts.get("draft"), Some(&1));
        assert_eq!(counts.get("title"), Some(&1));
    }
}
