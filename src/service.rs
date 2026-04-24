use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::github::graphql;
use crate::github::GitHubClient;
use crate::models::{ApprovalStatus, CiStatus, PrComment, PullRequest};

pub struct TrackedPullRequestSyncData {
    pub open_prs: Vec<PullRequest>,
    pub all_comments: Vec<PrComment>,
    pub closed_pr_numbers: Vec<i64>,
    pub max_updated_at: Option<DateTime<Utc>>,
}

pub async fn fetch_tracked_pull_requests_for_sync(
    github: &GitHubClient,
    repo_name: &str,
    authors_to_track: &[String],
    updated_after: Option<DateTime<Utc>>,
    username: &str,
) -> anyhow::Result<TrackedPullRequestSyncData> {
    let discovery_prs = github
        .fetch_tracked_pull_requests_search(repo_name, authors_to_track, updated_after)
        .await?;

    let closed_pr_numbers = discovery_prs
        .iter()
        .filter(|pr| pr.state != "OPEN")
        .map(|pr| pr.number)
        .collect();
    let max_updated_at = discovery_prs
        .iter()
        .filter_map(|pr| parse_github_timestamp(&pr.updated_at).ok())
        .max();
    let open_pr_numbers: Vec<i64> = discovery_prs
        .iter()
        .filter(|pr| pr.state == "OPEN")
        .map(|pr| pr.number)
        .collect();

    let open_prs =
        refresh_tracked_pull_requests_for_sync(github, repo_name, &open_pr_numbers, username)
            .await?;

    Ok(TrackedPullRequestSyncData {
        open_prs: open_prs.open_prs,
        all_comments: open_prs.all_comments,
        closed_pr_numbers,
        max_updated_at,
    })
}

pub async fn refresh_tracked_pull_requests_for_sync(
    github: &GitHubClient,
    repo_name: &str,
    pr_numbers: &[i64],
    username: &str,
) -> anyhow::Result<TrackedPullRequestSyncData> {
    let prs = github
        .fetch_pull_requests_by_numbers(repo_name, pr_numbers)
        .await?;

    process_tracked_pull_request_nodes(repo_name, &prs, username)
}

pub fn merge_tracked_pull_request_sync_data(
    discovery: TrackedPullRequestSyncData,
    refresh: TrackedPullRequestSyncData,
) -> TrackedPullRequestSyncData {
    let mut open_prs_by_number: HashMap<i64, PullRequest> = discovery
        .open_prs
        .into_iter()
        .map(|pr| (pr.number, pr))
        .collect();
    for pr in refresh.open_prs {
        open_prs_by_number.insert(pr.number, pr);
    }

    let mut comments_by_id: HashMap<String, PrComment> = discovery
        .all_comments
        .into_iter()
        .map(|comment| (comment.id.clone(), comment))
        .collect();
    for comment in refresh.all_comments {
        comments_by_id.insert(comment.id.clone(), comment);
    }

    let mut open_prs: Vec<PullRequest> = open_prs_by_number.into_values().collect();
    open_prs.sort_by_key(|pr| pr.number);

    let mut all_comments: Vec<PrComment> = comments_by_id.into_values().collect();
    all_comments.sort_by(|left, right| left.id.cmp(&right.id));

    TrackedPullRequestSyncData {
        open_prs,
        all_comments,
        closed_pr_numbers: discovery.closed_pr_numbers,
        max_updated_at: discovery.max_updated_at,
    }
}

fn process_tracked_pull_request_nodes(
    repo_name: &str,
    prs: &[graphql::PullRequestNode],
    username: &str,
) -> anyhow::Result<TrackedPullRequestSyncData> {
    let mut open_prs = Vec::new();
    let mut all_comments = Vec::new();
    let mut closed_pr_numbers = Vec::new();
    let mut max_updated_at = None;

    for pr in prs {
        let updated_at = parse_github_timestamp(&pr.updated_at)?;
        max_updated_at = Some(
            max_updated_at.map_or(updated_at, |current: DateTime<Utc>| current.max(updated_at)),
        );

        if pr.state == "OPEN" {
            let pr_model = graphql_pr_to_model(repo_name, pr, username)?;
            all_comments.extend(pr_model.comments.clone());
            open_prs.push(pr_model);
        } else {
            closed_pr_numbers.push(pr.number);
        }
    }

    Ok(TrackedPullRequestSyncData {
        open_prs,
        all_comments,
        closed_pr_numbers,
        max_updated_at,
    })
}

fn graphql_pr_to_model(
    repo_name: &str,
    pr: &graphql::PullRequestNode,
    username: &str,
) -> anyhow::Result<PullRequest> {
    let created_at = parse_github_timestamp(&pr.created_at)?;
    let updated_at = parse_github_timestamp(&pr.updated_at)?;

    let requested_reviewers = pr
        .review_requests
        .nodes
        .iter()
        .filter_map(|rr| rr.requested_reviewer.as_ref()?.login.clone())
        .collect();

    let user_has_reviewed = !username.is_empty()
        && pr.latest_reviews.nodes.iter().any(|review| {
            review
                .author
                .as_ref()
                .is_some_and(|a| a.login.eq_ignore_ascii_case(username))
        });

    let comments = map_comments_from_pr(repo_name, pr);

    Ok(PullRequest {
        number: pr.number,
        title: pr.title.clone(),
        repository: repo_name.to_string(),
        author: pr
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_default(),
        head_sha: pr.head_ref_oid.clone(),
        draft: pr.is_draft,
        created_at,
        updated_at,
        ci_status: map_ci_status(pr),
        last_comment_at: latest_comment_time(pr),
        last_commit_at: DateTime::UNIX_EPOCH,
        last_ci_status_update_at: DateTime::UNIX_EPOCH,
        approval_status: map_approval_status(pr),
        last_review_status_update_at: latest_review_submitted_at(pr),
        last_acknowledged_at: None,
        requested_reviewers,
        user_has_reviewed,
        comments,
    })
}

fn map_ci_status(pr: &graphql::PullRequestNode) -> CiStatus {
    let rollup = pr
        .commits
        .nodes
        .first()
        .and_then(|c| c.commit.status_check_rollup.as_ref());

    rollup.map_or(CiStatus::Success, map_rollup_ci_status)
}

fn map_rollup_ci_status(rollup: &graphql::StatusCheckRollup) -> CiStatus {
    let mut saw_required = false;
    let mut saw_pending = false;

    for context in &rollup.contexts.nodes {
        let (is_required, status) = map_status_check_rollup_context(context);
        if !is_required {
            continue;
        }

        saw_required = true;
        match status {
            CiStatus::Failure => return CiStatus::Failure,
            CiStatus::Pending => saw_pending = true,
            CiStatus::Success => {}
        }
    }

    if !saw_required {
        CiStatus::Success
    } else if saw_pending {
        CiStatus::Pending
    } else {
        CiStatus::Success
    }
}

fn map_status_check_rollup_context(
    context: &graphql::StatusCheckRollupContext,
) -> (bool, CiStatus) {
    match context {
        graphql::StatusCheckRollupContext::CheckRun {
            status,
            conclusion,
            is_required,
            ..
        } => (
            *is_required,
            map_check_run_status(status, conclusion.as_deref()),
        ),
        graphql::StatusCheckRollupContext::StatusContext {
            state, is_required, ..
        } => (*is_required, map_status_context_state(state)),
    }
}

fn map_check_run_status(status: &str, conclusion: Option<&str>) -> CiStatus {
    if status != "COMPLETED" {
        return CiStatus::Pending;
    }

    match conclusion {
        Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => CiStatus::Success,
        Some(
            "ACTION_REQUIRED" | "TIMED_OUT" | "CANCELLED" | "FAILURE" | "STARTUP_FAILURE" | "STALE",
        ) => CiStatus::Failure,
        _ => CiStatus::Pending,
    }
}

fn map_status_context_state(state: &str) -> CiStatus {
    match state {
        "SUCCESS" => CiStatus::Success,
        "FAILURE" | "ERROR" => CiStatus::Failure,
        _ => CiStatus::Pending,
    }
}

fn map_approval_status(pr: &graphql::PullRequestNode) -> ApprovalStatus {
    let mut has_approved = false;
    for review in &pr.latest_reviews.nodes {
        match review.state.as_str() {
            "CHANGES_REQUESTED" => return ApprovalStatus::ChangesRequested,
            "APPROVED" => has_approved = true,
            _ => {}
        }
    }
    if has_approved {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::None
    }
}

fn latest_review_submitted_at(pr: &graphql::PullRequestNode) -> DateTime<Utc> {
    pr.latest_reviews
        .nodes
        .iter()
        .filter_map(|r| parse_optional_timestamp(r.submitted_at.as_deref()))
        .max()
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn latest_comment_time(pr: &graphql::PullRequestNode) -> DateTime<Utc> {
    let comment_times = pr
        .comments
        .nodes
        .iter()
        .filter(|c| !is_bot_author(&c.author))
        .filter_map(|c| parse_optional_timestamp(Some(&c.updated_at)));

    let review_times = pr
        .reviews
        .nodes
        .iter()
        .filter(|r| !is_bot_author(&r.author))
        .filter_map(|r| parse_optional_timestamp(Some(&r.updated_at)));

    comment_times
        .chain(review_times)
        .max()
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn is_bot_author(author: &Option<graphql::Author>) -> bool {
    author
        .as_ref()
        .and_then(|author| author.actor_type.as_deref())
        .is_some_and(|actor_type| actor_type == "Bot")
}

fn map_comments_from_pr(repo_name: &str, pr: &graphql::PullRequestNode) -> Vec<PrComment> {
    let mut comments = Vec::new();

    for comment in pr
        .comments
        .nodes
        .iter()
        .filter(|comment| !is_bot_author(&comment.author))
    {
        let author = comment
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let created_at =
            parse_github_timestamp(&comment.created_at).unwrap_or(DateTime::UNIX_EPOCH);
        let updated_at =
            parse_github_timestamp(&comment.updated_at).unwrap_or(DateTime::UNIX_EPOCH);

        comments.push(PrComment {
            id: comment.id.clone(),
            repository: repo_name.to_string(),
            pr_number: pr.number,
            author,
            body: comment.body.clone(),
            created_at,
            updated_at,
            is_review_comment: false,
            review_state: None,
        });
    }

    for review in pr
        .reviews
        .nodes
        .iter()
        .filter(|review| !is_bot_author(&review.author))
    {
        let author = review
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let created_at = parse_github_timestamp(&review.created_at).unwrap_or(DateTime::UNIX_EPOCH);
        let updated_at = parse_github_timestamp(&review.updated_at).unwrap_or(DateTime::UNIX_EPOCH);

        comments.push(PrComment {
            id: review.id.clone(),
            repository: repo_name.to_string(),
            pr_number: pr.number,
            author,
            body: review.body.clone(),
            created_at,
            updated_at,
            is_review_comment: true,
            review_state: Some(review.state.clone()),
        });
    }

    comments
}

fn parse_github_timestamp(value: &str) -> anyhow::Result<DateTime<Utc>> {
    if value.is_empty() {
        return Ok(DateTime::UNIX_EPOCH);
    }

    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn parse_optional_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| parse_github_timestamp(raw).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::graphql::{
        Author, CommentConnection, CommentNode, CommitConnection, CommitDetail, CommitNode,
        LatestReviewConnection, LatestReviewNode, PullRequestNode, RequestedReviewer,
        ReviewConnection, ReviewNode, ReviewRequestConnection, ReviewRequestNode,
        StatusCheckRollup, StatusCheckRollupContext, StatusCheckRollupContextConnection,
    };

    fn test_pr(
        number: i64,
        state: &str,
        updated_at: &str,
        ci_state: Option<&str>,
    ) -> PullRequestNode {
        PullRequestNode {
            number,
            title: format!("PR {number}"),
            is_draft: false,
            created_at: "2025-06-15T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            state: state.to_string(),
            author: Some(Author {
                login: "alice".to_string(),
                actor_type: None,
            }),
            review_requests: ReviewRequestConnection { nodes: vec![] },
            head_ref_oid: "abc123".to_string(),
            commits: CommitConnection {
                nodes: vec![CommitNode {
                    commit: CommitDetail {
                        status_check_rollup: ci_state.map(|state| StatusCheckRollup {
                            state: state.to_string(),
                            contexts: StatusCheckRollupContextConnection::default(),
                        }),
                    },
                }],
            },
            comments: CommentConnection { nodes: vec![] },
            reviews: ReviewConnection { nodes: vec![] },
            latest_reviews: LatestReviewConnection { nodes: vec![] },
        }
    }

    fn test_pr_with_reviews(review_states: &[&str]) -> PullRequestNode {
        let mut pr = test_pr(42, "OPEN", "2025-06-15T00:00:00Z", Some("SUCCESS"));
        pr.latest_reviews = LatestReviewConnection {
            nodes: review_states
                .iter()
                .enumerate()
                .map(|(index, state)| LatestReviewNode {
                    state: (*state).to_string(),
                    submitted_at: Some(format!("2025-06-15T00:00:0{}Z", index + 1)),
                    author: Some(Author {
                        login: "reviewer".to_string(),
                        actor_type: None,
                    }),
                })
                .collect(),
        };
        pr
    }

    #[test]
    fn map_rollup_ci_status_maps_success() {
        let rollup = StatusCheckRollup {
            state: "SUCCESS".to_string(),
            contexts: StatusCheckRollupContextConnection {
                nodes: vec![StatusCheckRollupContext::CheckRun {
                    name: "build".to_string(),
                    status: "COMPLETED".to_string(),
                    conclusion: Some("SUCCESS".to_string()),
                    is_required: true,
                }],
            },
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Success);
    }

    #[test]
    fn map_rollup_ci_status_maps_failure() {
        let rollup = StatusCheckRollup {
            state: "FAILURE".to_string(),
            contexts: StatusCheckRollupContextConnection {
                nodes: vec![StatusCheckRollupContext::CheckRun {
                    name: "build".to_string(),
                    status: "COMPLETED".to_string(),
                    conclusion: Some("FAILURE".to_string()),
                    is_required: true,
                }],
            },
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Failure);
    }

    #[test]
    fn map_rollup_ci_status_maps_pending_for_other_states() {
        let rollup = StatusCheckRollup {
            state: "PENDING".to_string(),
            contexts: StatusCheckRollupContextConnection {
                nodes: vec![StatusCheckRollupContext::CheckRun {
                    name: "build".to_string(),
                    status: "IN_PROGRESS".to_string(),
                    conclusion: None,
                    is_required: true,
                }],
            },
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Pending);
    }

    #[test]
    fn map_rollup_ci_status_ignores_optional_failures() {
        let rollup = StatusCheckRollup {
            state: "FAILURE".to_string(),
            contexts: StatusCheckRollupContextConnection {
                nodes: vec![StatusCheckRollupContext::CheckRun {
                    name: "optional".to_string(),
                    status: "COMPLETED".to_string(),
                    conclusion: Some("FAILURE".to_string()),
                    is_required: false,
                }],
            },
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Success);
    }

    #[test]
    fn map_rollup_ci_status_returns_success_when_no_required_checks_exist() {
        let rollup = StatusCheckRollup {
            state: "PENDING".to_string(),
            contexts: StatusCheckRollupContextConnection { nodes: vec![] },
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Success);
    }

    #[test]
    fn process_tracked_pull_request_nodes_splits_open_and_closed() {
        let prs = vec![
            test_pr(1, "OPEN", "2025-06-15T00:00:00Z", Some("SUCCESS")),
            test_pr(2, "MERGED", "2025-06-16T00:00:00Z", Some("FAILURE")),
            test_pr(3, "CLOSED", "2025-06-14T00:00:00Z", None),
        ];

        let result = process_tracked_pull_request_nodes("owner/repo", &prs, "alice")
            .expect("processing succeeds");

        assert_eq!(result.open_prs.len(), 1);
        assert_eq!(result.open_prs[0].number, 1);
        assert_eq!(result.closed_pr_numbers, vec![2, 3]);
        assert_eq!(
            result.max_updated_at,
            parse_github_timestamp("2025-06-16T00:00:00Z").ok()
        );
    }

    #[test]
    fn process_tracked_pull_request_nodes_collects_comments() {
        let mut pr = test_pr(1, "OPEN", "2025-06-15T00:00:00Z", Some("SUCCESS"));
        pr.comments = CommentConnection {
            nodes: vec![CommentNode {
                id: "comment-1".to_string(),
                author: Some(Author {
                    login: "alice".to_string(),
                    actor_type: None,
                }),
                body: "hello".to_string(),
                created_at: "2025-06-15T00:00:00Z".to_string(),
                updated_at: "2025-06-15T00:01:00Z".to_string(),
            }],
        };
        pr.reviews = ReviewConnection {
            nodes: vec![ReviewNode {
                id: "review-1".to_string(),
                author: Some(Author {
                    login: "bob".to_string(),
                    actor_type: None,
                }),
                body: "looks good".to_string(),
                created_at: "2025-06-15T00:02:00Z".to_string(),
                updated_at: "2025-06-15T00:03:00Z".to_string(),
                state: "APPROVED".to_string(),
                submitted_at: Some("2025-06-15T00:03:00Z".to_string()),
            }],
        };

        let result = process_tracked_pull_request_nodes("owner/repo", &[pr], "alice")
            .expect("processing succeeds");

        assert_eq!(result.all_comments.len(), 2);
        assert_eq!(
            result.open_prs[0].last_comment_at,
            parse_github_timestamp("2025-06-15T00:03:00Z").unwrap()
        );
    }

    #[test]
    fn process_tracked_pull_request_nodes_ignores_bot_comments() {
        let mut pr = test_pr(1, "OPEN", "2025-06-15T00:00:00Z", Some("SUCCESS"));
        pr.comments = CommentConnection {
            nodes: vec![
                CommentNode {
                    id: "bot-comment".to_string(),
                    author: Some(Author {
                        login: "github-actions".to_string(),
                        actor_type: Some("Bot".to_string()),
                    }),
                    body: "generated output".to_string(),
                    created_at: "2025-06-15T00:00:00Z".to_string(),
                    updated_at: "2025-06-15T00:10:00Z".to_string(),
                },
                CommentNode {
                    id: "human-comment".to_string(),
                    author: Some(Author {
                        login: "juliehockey30".to_string(),
                        actor_type: Some("User".to_string()),
                    }),
                    body: "real feedback".to_string(),
                    created_at: "2025-06-15T00:01:00Z".to_string(),
                    updated_at: "2025-06-15T00:02:00Z".to_string(),
                },
            ],
        };
        pr.reviews = ReviewConnection {
            nodes: vec![ReviewNode {
                id: "bot-review".to_string(),
                author: Some(Author {
                    login: "claude".to_string(),
                    actor_type: Some("Bot".to_string()),
                }),
                body: "automated review".to_string(),
                created_at: "2025-06-15T00:03:00Z".to_string(),
                updated_at: "2025-06-15T00:11:00Z".to_string(),
                state: "COMMENTED".to_string(),
                submitted_at: Some("2025-06-15T00:11:00Z".to_string()),
            }],
        };

        let result = process_tracked_pull_request_nodes("owner/repo", &[pr], "alice")
            .expect("processing succeeds");

        assert_eq!(result.all_comments.len(), 1);
        assert_eq!(result.all_comments[0].author, "juliehockey30");
        assert_eq!(
            result.open_prs[0].last_comment_at,
            parse_github_timestamp("2025-06-15T00:02:00Z").unwrap()
        );
    }

    #[test]
    fn map_approval_status_prefers_changes_requested() {
        let pr = test_pr_with_reviews(&["APPROVED", "CHANGES_REQUESTED"]);

        assert_eq!(map_approval_status(&pr), ApprovalStatus::ChangesRequested);
    }

    #[test]
    fn graphql_pr_to_model_maps_requested_reviewers_and_user_reviewed() {
        let mut pr = test_pr_with_reviews(&["APPROVED"]);
        pr.review_requests = ReviewRequestConnection {
            nodes: vec![ReviewRequestNode {
                requested_reviewer: Some(RequestedReviewer {
                    login: Some("carol".to_string()),
                }),
            }],
        };
        pr.latest_reviews = LatestReviewConnection {
            nodes: vec![LatestReviewNode {
                state: "APPROVED".to_string(),
                submitted_at: Some("2025-06-15T00:00:01Z".to_string()),
                author: Some(Author {
                    login: "alice".to_string(),
                    actor_type: None,
                }),
            }],
        };

        let model = graphql_pr_to_model("owner/repo", &pr, "Alice").expect("mapping succeeds");

        assert_eq!(model.requested_reviewers, vec!["carol".to_string()]);
        assert!(model.user_has_reviewed);
    }

    #[test]
    fn merge_tracked_pull_request_sync_data_prefers_refresh_copy() {
        let mut discovery = process_tracked_pull_request_nodes(
            "owner/repo",
            &[test_pr(1, "OPEN", "2025-06-15T00:00:00Z", Some("FAILURE"))],
            "alice",
        )
        .expect("processing succeeds");
        let refresh = process_tracked_pull_request_nodes(
            "owner/repo",
            &[test_pr(1, "OPEN", "2025-06-15T00:00:00Z", Some("SUCCESS"))],
            "alice",
        )
        .expect("processing succeeds");

        discovery.closed_pr_numbers = vec![99];
        let merged = merge_tracked_pull_request_sync_data(discovery, refresh);

        assert_eq!(merged.open_prs.len(), 1);
        assert_eq!(merged.open_prs[0].ci_status, CiStatus::Success);
        assert_eq!(merged.closed_pr_numbers, vec![99]);
    }
}
