use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::github::graphql;
use crate::github::GitHubClient;
use crate::models::{ApprovalStatus, CiStatus, PrComment, PullRequest};

pub async fn fetch_tracked_pull_requests(
    github: &GitHubClient,
    repo_name: &str,
    authors_to_track: &[String],
    updated_after: Option<DateTime<Utc>>,
    username: &str,
) -> anyhow::Result<(Vec<PullRequest>, Vec<PrComment>)> {
    let prs = github
        .fetch_open_pull_requests_graphql(repo_name, updated_after)
        .await?;

    let mut pull_requests = Vec::new();
    let mut all_comments = Vec::new();

    for pr in &prs {
        let author = pr
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_default();

        if !authors_to_track.iter().any(|tracked| tracked == &author) {
            continue;
        }

        let pr_model = graphql_pr_to_model(repo_name, pr, username)?;
        all_comments.extend(pr_model.comments.clone());
        pull_requests.push(pr_model);
    }

    Ok((pull_requests, all_comments))
}

pub async fn discover_new_pull_requests(
    github: &GitHubClient,
    repo_name: &str,
    authors_to_track: &[String],
    known_pr_numbers: &[i64],
    updated_after: Option<DateTime<Utc>>,
) -> anyhow::Result<(Vec<i64>, Option<DateTime<Utc>>)> {
    let prs = github
        .fetch_discovery_pull_requests_graphql(repo_name, updated_after)
        .await?;

    let max_updated_at = prs
        .iter()
        .filter_map(|pr| parse_github_timestamp(&pr.updated_at).ok())
        .max();

    let known: HashSet<i64> = known_pr_numbers.iter().copied().collect();
    let new_pr_numbers = filter_new_prs(&prs, authors_to_track, &known);

    Ok((new_pr_numbers, max_updated_at))
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

    let pr_model = PullRequest {
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
    };

    Ok(pr_model)
}

fn map_ci_status(pr: &graphql::PullRequestNode) -> CiStatus {
    let rollup = pr
        .commits
        .nodes
        .first()
        .and_then(|c| c.commit.status_check_rollup.as_ref());

    match rollup {
        Some(r) => map_rollup_ci_status(r),
        None => CiStatus::Pending,
    }
}

fn map_rollup_ci_status(rollup: &graphql::StatusCheckRollup) -> CiStatus {
    let Some(contexts) = rollup.contexts.as_ref() else {
        return fallback_rollup_state(&rollup.state);
    };

    let mut has_required = false;
    let mut has_required_pending = false;

    for context in &contexts.nodes {
        match required_context_state(context) {
            Some(RequiredContextState::Failure) => return CiStatus::Failure,
            Some(RequiredContextState::Pending) => {
                has_required = true;
                has_required_pending = true;
            }
            Some(RequiredContextState::Success) => {
                has_required = true;
            }
            None => {}
        }
    }

    if !has_required {
        CiStatus::Success
    } else if has_required_pending {
        CiStatus::Pending
    } else {
        CiStatus::Success
    }
}

fn fallback_rollup_state(state: &str) -> CiStatus {
    match state {
        "SUCCESS" => CiStatus::Success,
        "FAILURE" | "ERROR" => CiStatus::Failure,
        _ => CiStatus::Pending,
    }
}

enum RequiredContextState {
    Success,
    Pending,
    Failure,
}

fn required_context_state(
    context: &graphql::StatusCheckRollupContext,
) -> Option<RequiredContextState> {
    match context {
        graphql::StatusCheckRollupContext::CheckRun {
            status,
            conclusion,
            is_required,
            ..
        } => {
            if !is_required {
                return None;
            }

            if status != "COMPLETED" {
                return Some(RequiredContextState::Pending);
            }

            match conclusion.as_deref() {
                Some("SUCCESS") | Some("NEUTRAL") | Some("SKIPPED") => {
                    Some(RequiredContextState::Success)
                }
                Some("FAILURE")
                | Some("TIMED_OUT")
                | Some("ACTION_REQUIRED")
                | Some("STARTUP_FAILURE")
                | Some("CANCELLED")
                | Some("STALE") => Some(RequiredContextState::Failure),
                _ => Some(RequiredContextState::Pending),
            }
        }
        graphql::StatusCheckRollupContext::StatusContext {
            state, is_required, ..
        } => {
            if !is_required {
                return None;
            }

            match state.as_str() {
                "SUCCESS" => Some(RequiredContextState::Success),
                "ERROR" | "FAILURE" => Some(RequiredContextState::Failure),
                _ => Some(RequiredContextState::Pending),
            }
        }
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
        .filter_map(|c| parse_optional_timestamp(Some(&c.updated_at)));

    let review_times = pr
        .reviews
        .nodes
        .iter()
        .filter_map(|r| parse_optional_timestamp(Some(&r.updated_at)));

    comment_times
        .chain(review_times)
        .max()
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn map_comments_from_pr(repo_name: &str, pr: &graphql::PullRequestNode) -> Vec<PrComment> {
    let mut comments = Vec::new();

    // Extract issue comments from pr.comments.nodes
    for comment in &pr.comments.nodes {
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

    // Extract review comments from pr.reviews.nodes
    for review in &pr.reviews.nodes {
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

fn filter_new_prs(
    prs: &[graphql::DiscoveryPullRequestNode],
    authors_to_track: &[String],
    known_pr_numbers: &HashSet<i64>,
) -> Vec<i64> {
    let tracked_authors: HashSet<String> = authors_to_track
        .iter()
        .map(|author| author.to_ascii_lowercase())
        .collect();

    prs.iter()
        .filter(|pr| {
            let author = pr
                .author
                .as_ref()
                .map(|a| a.login.to_ascii_lowercase())
                .unwrap_or_default();

            tracked_authors.contains(&author) && !known_pr_numbers.contains(&pr.number)
        })
        .map(|pr| pr.number)
        .collect()
}

pub async fn fetch_pull_requests_by_number(
    github: &GitHubClient,
    repo_name: &str,
    pr_numbers: &[i64],
    username: &str,
) -> anyhow::Result<(Vec<PullRequest>, Vec<PrComment>, Vec<i64>)> {
    let results = github
        .fetch_pull_requests_by_number(repo_name, pr_numbers)
        .await?;

    let mut open_prs = Vec::new();
    let mut all_comments = Vec::new();
    let mut closed_pr_numbers = Vec::new();

    for (number, maybe_node) in results {
        match maybe_node {
            Some(ref node) if node.state.as_deref() == Some("OPEN") => {
                let pr_model = graphql_pr_to_model(repo_name, node, username)?;
                all_comments.extend(pr_model.comments.clone());
                open_prs.push(pr_model);
            }
            _ => {
                closed_pr_numbers.push(number);
            }
        }
    }

    Ok((open_prs, all_comments, closed_pr_numbers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::graphql::{
        Author, DiscoveryPullRequestNode, StatusCheckRollup, StatusCheckRollupContext,
        StatusCheckRollupContextConnection,
    };

    fn check_run(
        is_required: bool,
        status: &str,
        conclusion: Option<&str>,
    ) -> StatusCheckRollupContext {
        StatusCheckRollupContext::CheckRun {
            name: "test".to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(str::to_string),
            is_required,
        }
    }

    fn status_context(is_required: bool, state: &str) -> StatusCheckRollupContext {
        StatusCheckRollupContext::StatusContext {
            context: "legacy".to_string(),
            state: state.to_string(),
            is_required,
        }
    }

    fn rollup_with_contexts(contexts: Vec<StatusCheckRollupContext>) -> StatusCheckRollup {
        StatusCheckRollup {
            state: "PENDING".to_string(),
            contexts: Some(StatusCheckRollupContextConnection { nodes: contexts }),
        }
    }

    fn discovery_pr(number: i64, author: Option<&str>) -> DiscoveryPullRequestNode {
        DiscoveryPullRequestNode {
            number,
            updated_at: "2025-06-15T00:00:00Z".to_string(),
            author: author.map(|login| Author {
                login: login.to_string(),
            }),
        }
    }

    #[test]
    fn filter_new_prs_includes_tracked_author_unknown_pr() {
        let prs = vec![discovery_pr(42, Some("alice"))];
        let authors = vec!["alice".to_string()];
        let known = HashSet::new();

        let result = filter_new_prs(&prs, &authors, &known);
        assert_eq!(result, vec![42]);
    }

    #[test]
    fn filter_new_prs_excludes_untracked_author() {
        let prs = vec![discovery_pr(42, Some("bob"))];
        let authors = vec!["alice".to_string()];
        let known = HashSet::new();

        let result = filter_new_prs(&prs, &authors, &known);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_new_prs_excludes_known_pr() {
        let prs = vec![discovery_pr(42, Some("alice"))];
        let authors = vec!["alice".to_string()];
        let known: HashSet<i64> = [42].into_iter().collect();

        let result = filter_new_prs(&prs, &authors, &known);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_new_prs_excludes_pr_with_no_author() {
        let prs = vec![discovery_pr(42, None)];
        let authors = vec!["alice".to_string()];
        let known = HashSet::new();

        let result = filter_new_prs(&prs, &authors, &known);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_new_prs_mixed() {
        let prs = vec![
            discovery_pr(1, Some("alice")), // tracked, new
            discovery_pr(2, Some("bob")),   // untracked
            discovery_pr(3, Some("alice")), // tracked, but known
            discovery_pr(4, Some("carol")), // tracked, new
        ];
        let authors = vec!["alice".to_string(), "carol".to_string()];
        let known: HashSet<i64> = [3].into_iter().collect();

        let result = filter_new_prs(&prs, &authors, &known);
        assert_eq!(result, vec![1, 4]);
    }

    #[test]
    fn filter_new_prs_matches_authors_case_insensitively() {
        let prs = vec![discovery_pr(42, Some("Alice"))];
        let authors = vec!["alice".to_string()];
        let known = HashSet::new();

        let result = filter_new_prs(&prs, &authors, &known);
        assert_eq!(result, vec![42]);
    }

    #[test]
    fn map_rollup_ci_status_returns_success_when_no_required_checks_exist() {
        let rollup = rollup_with_contexts(vec![
            check_run(false, "COMPLETED", Some("FAILURE")),
            status_context(false, "FAILURE"),
        ]);

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Success);
    }

    #[test]
    fn map_rollup_ci_status_returns_failure_for_required_failed_check() {
        let rollup = rollup_with_contexts(vec![
            check_run(false, "COMPLETED", Some("FAILURE")),
            check_run(true, "COMPLETED", Some("FAILURE")),
        ]);

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Failure);
    }

    #[test]
    fn map_rollup_ci_status_returns_pending_for_required_in_progress_check() {
        let rollup = rollup_with_contexts(vec![
            check_run(true, "IN_PROGRESS", None),
            check_run(false, "COMPLETED", Some("FAILURE")),
        ]);

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Pending);
    }

    #[test]
    fn map_rollup_ci_status_returns_success_for_all_required_successful_checks() {
        let rollup = rollup_with_contexts(vec![
            check_run(true, "COMPLETED", Some("SUCCESS")),
            status_context(true, "SUCCESS"),
            check_run(false, "COMPLETED", Some("FAILURE")),
        ]);

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Success);
    }

    #[test]
    fn map_rollup_ci_status_uses_rollup_state_when_contexts_are_missing() {
        let rollup = StatusCheckRollup {
            state: "FAILURE".to_string(),
            contexts: None,
        };

        assert_eq!(map_rollup_ci_status(&rollup), CiStatus::Failure);
    }
}
