use chrono::{DateTime, Utc};

use crate::github::schema;
use crate::github::GitHubClient;
use crate::models::{CiStatus, PullRequest};

pub async fn fetch_pull_request_details(
    github: &GitHubClient,
    repo_name: &str,
    pr_id: i64,
) -> anyhow::Result<PullRequest> {
    let pr_details = github.fetch_pull_request_details(repo_name, pr_id).await?;
    let ci_statuses = github
        .fetch_pull_request_ci_statuses(repo_name, pr_id)
        .await?;

    let created_at = parse_github_timestamp(&pr_details.pull_request.created_at)?;
    let updated_at = parse_github_timestamp(&pr_details.pull_request.updated_at)?;
    let pull_request = &pr_details.pull_request;

    let requested_reviewers = pull_request
        .requested_reviewers
        .iter()
        .map(|r| r.login.clone())
        .collect();

    Ok(PullRequest {
        number: pull_request.number,
        title: pull_request.title.clone(),
        repository: repo_name.to_string(),
        author: pull_request.user.login.clone(),
        head_sha: ci_statuses.head_sha.clone(),
        draft: pull_request.draft,
        created_at,
        updated_at,
        ci_status: map_ci_status(&ci_statuses),
        last_comment_at: latest_comment_time(&pr_details),
        last_commit_at: DateTime::UNIX_EPOCH,
        last_ci_status_update_at: DateTime::UNIX_EPOCH,
        last_acknowledged_at: None,
        requested_reviewers,
    })
}

pub async fn fetch_tracked_pull_requests(
    github: &GitHubClient,
    repo_name: &str,
    authors_to_track: &[String],
) -> anyhow::Result<Vec<PullRequest>> {
    let prs = github.fetch_open_pull_requests(repo_name).await?;
    let mut result = Vec::new();

    for pr in prs {
        if !should_track_pr(&pr, authors_to_track) {
            continue;
        }

        let details = fetch_pull_request_details(github, repo_name, pr.number).await?;
        result.push(details);
    }

    Ok(result)
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

fn latest_comment_time(pr_details: &schema::PullRequestDetails) -> DateTime<Utc> {
    let issue_latest = pr_details
        .issue_comments
        .iter()
        .filter_map(|comment| parse_optional_timestamp(Some(comment.updated_at.as_str())));
    let review_latest = pr_details
        .review_comments
        .iter()
        .filter_map(|comment| parse_optional_timestamp(Some(comment.updated_at.as_str())));

    issue_latest
        .chain(review_latest)
        .max()
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn map_ci_status(ci_statuses: &schema::PullRequestCiStatuses) -> CiStatus {
    if has_failing_check_run(&ci_statuses.check_runs) {
        return CiStatus::Failure;
    }

    if has_pending_check_run(&ci_statuses.check_runs) {
        return CiStatus::Pending;
    }

    match ci_statuses.combined_state.as_str() {
        "success" => CiStatus::Success,
        "failure" | "error" => CiStatus::Failure,
        _ => CiStatus::Pending,
    }
}

fn has_failing_check_run(check_runs: &[schema::CheckRun]) -> bool {
    check_runs
        .iter()
        .filter_map(|run| run.conclusion.as_deref())
        .any(|conclusion| {
            matches!(
                conclusion,
                "failure"
                    | "timed_out"
                    | "cancelled"
                    | "startup_failure"
                    | "action_required"
                    | "stale"
            )
        })
}

fn has_pending_check_run(check_runs: &[schema::CheckRun]) -> bool {
    check_runs.iter().any(|run| {
        matches!(
            run.status.as_str(),
            "queued" | "in_progress" | "waiting" | "requested" | "pending"
        )
    })
}

fn should_track_pr(pr: &schema::PullRequest, authors_to_track: &[String]) -> bool {
    authors_to_track
        .iter()
        .any(|author| author == &pr.user.login)
}
