use chrono::{DateTime, Utc};

use crate::github::graphql;
use crate::github::GitHubClient;
use crate::models::{CiStatus, PullRequest};

pub async fn fetch_tracked_pull_requests(
    github: &GitHubClient,
    repo_name: &str,
    authors_to_track: &[String],
) -> anyhow::Result<Vec<PullRequest>> {
    let prs = github.fetch_open_pull_requests_graphql(repo_name).await?;

    let mut result = Vec::new();
    for pr in &prs {
        let author = pr
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_default();

        if !authors_to_track.iter().any(|tracked| tracked == &author) {
            continue;
        }

        let model = graphql_pr_to_model(repo_name, pr)?;
        result.push(model);
    }

    Ok(result)
}

fn graphql_pr_to_model(
    repo_name: &str,
    pr: &graphql::PullRequestNode,
) -> anyhow::Result<PullRequest> {
    let created_at = parse_github_timestamp(&pr.created_at)?;
    let updated_at = parse_github_timestamp(&pr.updated_at)?;

    let requested_reviewers = pr
        .review_requests
        .nodes
        .iter()
        .filter_map(|rr| rr.requested_reviewer.as_ref()?.login.clone())
        .collect();

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
        last_acknowledged_at: None,
        requested_reviewers,
    })
}

fn map_ci_status(pr: &graphql::PullRequestNode) -> CiStatus {
    let rollup = pr
        .commits
        .nodes
        .first()
        .and_then(|c| c.commit.status_check_rollup.as_ref());

    match rollup {
        Some(r) => match r.state.as_str() {
            "SUCCESS" => CiStatus::Success,
            "FAILURE" | "ERROR" => CiStatus::Failure,
            _ => CiStatus::Pending,
        },
        None => CiStatus::Pending,
    }
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

fn parse_github_timestamp(value: &str) -> anyhow::Result<DateTime<Utc>> {
    if value.is_empty() {
        return Ok(DateTime::UNIX_EPOCH);
    }

    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn parse_optional_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| parse_github_timestamp(raw).ok())
}
