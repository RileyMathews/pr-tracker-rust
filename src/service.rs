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
        Some(r) => match r.state.as_str() {
            "SUCCESS" => CiStatus::Success,
            "FAILURE" | "ERROR" => CiStatus::Failure,
            _ => CiStatus::Pending,
        },
        None => CiStatus::Pending,
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
    prs.iter()
        .filter(|pr| {
            let author = pr
                .author
                .as_ref()
                .map(|a| a.login.as_str())
                .unwrap_or_default();
            authors_to_track.iter().any(|tracked| tracked == author)
                && !known_pr_numbers.contains(&pr.number)
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
    use crate::github::graphql::{Author, DiscoveryPullRequestNode};

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
}
