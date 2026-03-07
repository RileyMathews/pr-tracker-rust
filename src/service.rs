use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::github::mappers::{filter_new_prs, graphql_pr_to_model, parse_github_timestamp};
use crate::github::GitHubClient;
use crate::models::{PrComment, PullRequest};

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
