use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::core::{process_pull_request_sync_results, SyncDiff};
use crate::db::DatabaseRepository;
use crate::github::GitHubClient;
use crate::models::{PullRequest, TrackedRepository};
use crate::service;

const DEFAULT_MAX_PR_AGE_DAYS: i64 = 7;
const MAX_CONCURRENT_REPOS: usize = 5;

fn pr_age_cutoff() -> Option<DateTime<Utc>> {
    let days: i64 = std::env::var("PR_TRACKER_MAX_PR_AGE_DAYS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_MAX_PR_AGE_DAYS);

    if days <= 0 {
        return None; // 0 or negative means no cutoff (fetch all)
    }

    Some(Utc::now() - chrono::Duration::days(days))
}

/// Compute the discovery cutoff for a repository.
/// Uses last_synced_at if available; falls back to pr_age_cutoff().
/// If both exist, uses the more recent (tighter) one.
fn compute_discovery_cutoff(
    last_synced_at: Option<DateTime<Utc>>,
    age_cutoff: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (last_synced_at, age_cutoff) {
        (Some(last_synced), Some(age)) => Some(last_synced.max(age)),
        (Some(last_synced), None) => Some(last_synced),
        (None, age) => age,
    }
}

fn effective_tracked_authors(tracked_authors: &[String], username: &str) -> Vec<String> {
    let mut authors = tracked_authors.to_vec();

    if !username.is_empty()
        && !authors
            .iter()
            .any(|author| author.eq_ignore_ascii_case(username))
    {
        authors.push(username.to_string());
    }

    authors
}

#[derive(Debug, Default)]
pub struct SyncRunSummary {
    pub synced_repositories: usize,
    pub new_prs: Vec<PullRequest>,
    pub updated_prs: Vec<PullRequest>,
    pub deleted_prs: Vec<PullRequest>,
}

#[derive(Debug, Clone)]
pub enum SyncProgress {
    FullSyncStarted {
        total_repositories: usize,
    },
    FullSyncRepositoryStarted {
        repository: String,
        repository_index: usize,
        total_repositories: usize,
    },
    FullSyncRepositoryCompleted {
        repository: String,
        repository_index: usize,
        total_repositories: usize,
        new_prs: usize,
        updated_prs: usize,
        deleted_prs: usize,
    },
}

struct RepoSyncResult {
    repo_name: String,
    repo_index: usize,
    new_prs: Vec<PullRequest>,
    updated_prs: Vec<PullRequest>,
    deleted_prs: Vec<PullRequest>,
}

pub async fn sync_all_tracked(
    repository: &DatabaseRepository,
    github: &GitHubClient,
    username: &str,
) -> anyhow::Result<SyncRunSummary> {
    sync_all_tracked_with_progress(repository, github, username, |_| {}).await
}

pub async fn sync_all_tracked_with_progress<F>(
    repository: &DatabaseRepository,
    github: &GitHubClient,
    username: &str,
    mut progress_callback: F,
) -> anyhow::Result<SyncRunSummary>
where
    F: FnMut(SyncProgress),
{
    let repositories = repository.get_tracked_repositories().await?;
    let tracked_authors =
        effective_tracked_authors(&repository.get_tracked_authors().await?, username);

    let mut summary = SyncRunSummary::default();
    progress_callback(SyncProgress::FullSyncStarted {
        total_repositories: repositories.len(),
    });

    if repositories.is_empty() || tracked_authors.is_empty() {
        return Ok(summary);
    }

    let total_repositories = repositories.len();
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REPOS));
    let mut join_set = JoinSet::new();

    let username_owned = username.to_string();

    for (index, tracked_repo) in repositories.into_iter().enumerate() {
        progress_callback(SyncProgress::FullSyncRepositoryStarted {
            repository: tracked_repo.repository.clone(),
            repository_index: index + 1,
            total_repositories,
        });

        let sem = semaphore.clone();
        let db = repository.clone();
        let gh = github.clone();
        let authors = tracked_authors.clone();
        let uname = username_owned.clone();

        join_set.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            sync_single_repo(&db, &gh, &authors, tracked_repo, index + 1, &uname).await
        });
    }

    while let Some(result) = join_set.join_next().await {
        let repo_result = result??;
        progress_callback(SyncProgress::FullSyncRepositoryCompleted {
            repository: repo_result.repo_name.clone(),
            repository_index: repo_result.repo_index,
            total_repositories,
            new_prs: repo_result.new_prs.len(),
            updated_prs: repo_result.updated_prs.len(),
            deleted_prs: repo_result.deleted_prs.len(),
        });
        summary.synced_repositories += 1;
        summary.new_prs.extend(repo_result.new_prs);
        summary.updated_prs.extend(repo_result.updated_prs);
        summary.deleted_prs.extend(repo_result.deleted_prs);
    }

    Ok(summary)
}

async fn sync_single_repo(
    repository: &DatabaseRepository,
    github: &GitHubClient,
    tracked_authors: &[String],
    tracked_repo: TrackedRepository,
    repo_index: usize,
    username: &str,
) -> anyhow::Result<RepoSyncResult> {
    let repo_name = &tracked_repo.repository;

    // Step 1: Compute cutoff
    let discovery_cutoff = compute_discovery_cutoff(tracked_repo.last_synced_at, pr_age_cutoff());

    // Step 2: Phase 1 — Discovery
    let existing_prs = repository.get_prs_by_repository(repo_name).await?;
    let known_pr_numbers: Vec<i64> = existing_prs.iter().map(|pr| pr.number).collect();

    let (new_pr_numbers, max_updated_at) = service::discover_new_pull_requests(
        github,
        repo_name,
        tracked_authors,
        &known_pr_numbers,
        discovery_cutoff,
    )
    .await?;

    // Step 3: Phase 2 — Targeted Refresh
    // Collect ALL PR numbers to refresh: existing + newly discovered
    let mut all_pr_numbers = known_pr_numbers;
    all_pr_numbers.extend(&new_pr_numbers);

    let (fresh_prs, all_comments, closed_pr_numbers) = if all_pr_numbers.is_empty() {
        (Vec::new(), Vec::new(), Vec::new())
    } else {
        service::fetch_pull_requests_by_number(github, repo_name, &all_pr_numbers, username).await?
    };

    // Step 4: Diff & persist
    // Use process_pull_request_sync_results for open PRs (same as before)
    // Pass ALL existing PRs — no cutoff filtering needed anymore since
    // closed PR detection is explicit via the state field in Phase 2
    let SyncDiff {
        new_prs,
        updated_prs,
        removed_prs: _, // We handle removals via closed_pr_numbers instead
    } = process_pull_request_sync_results(&existing_prs, &fresh_prs, Utc::now());

    for pr in &new_prs {
        repository.save_pr(pr).await?;
    }
    for pr in &updated_prs {
        repository.save_pr(pr).await?;
    }

    // Delete closed/merged/deleted PRs explicitly
    for pr_number in &closed_pr_numbers {
        repository.delete_pr(repo_name, *pr_number).await?;
    }

    // Persist comments for all fetched PRs
    for comment in all_comments {
        repository.save_comment(&comment).await?;
    }

    // Step 5: Update last_synced_at
    // Store the GitHub-side timestamp watermark (not local clock)
    if let Some(max_ts) = max_updated_at {
        // Subtract 1 second to create a small overlap window, ensuring PRs
        // updated at exactly the watermark timestamp are re-scanned next time.
        // These PRs will already be in our DB from this sync, so the overlap
        // just causes them to appear in discovery (where they'll be filtered
        // out as known PRs).
        let watermark = max_ts - chrono::Duration::seconds(1);
        repository
            .update_tracked_repository_last_synced_at(repo_name, watermark)
            .await?;
    }

    // Step 6: Build result
    let closed_set: HashSet<i64> = closed_pr_numbers.iter().copied().collect();
    let deleted_prs: Vec<PullRequest> = existing_prs
        .into_iter()
        .filter(|pr| closed_set.contains(&pr.number))
        .collect();

    Ok(RepoSyncResult {
        repo_name: repo_name.clone(),
        repo_index,
        new_prs,
        updated_prs,
        deleted_prs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
            .single()
            .expect("valid datetime")
    }

    #[test]
    fn cutoff_uses_last_synced_when_more_recent() {
        let last_synced = dt(2025, 6, 15); // June 15
        let age = dt(2025, 6, 10); // June 10
        let result = compute_discovery_cutoff(Some(last_synced), Some(age));
        assert_eq!(result, Some(last_synced)); // last_synced is more recent
    }

    #[test]
    fn cutoff_uses_age_when_more_recent() {
        let last_synced = dt(2025, 6, 1); // June 1 (old sync)
        let age = dt(2025, 6, 10); // June 10
        let result = compute_discovery_cutoff(Some(last_synced), Some(age));
        assert_eq!(result, Some(age)); // age cutoff is more recent
    }

    #[test]
    fn cutoff_uses_last_synced_when_no_age() {
        let last_synced = dt(2025, 6, 15);
        let result = compute_discovery_cutoff(Some(last_synced), None);
        assert_eq!(result, Some(last_synced));
    }

    #[test]
    fn cutoff_uses_age_when_no_last_synced() {
        let age = dt(2025, 6, 10);
        let result = compute_discovery_cutoff(None, Some(age));
        assert_eq!(result, Some(age));
    }

    #[test]
    fn cutoff_is_none_when_both_none() {
        let result = compute_discovery_cutoff(None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn effective_tracked_authors_includes_current_user() {
        let authors = vec!["alice".to_string()];

        let result = effective_tracked_authors(&authors, "bob");

        assert_eq!(result, vec!["alice".to_string(), "bob".to_string()]);
    }

    #[test]
    fn effective_tracked_authors_avoids_case_insensitive_duplicates() {
        let authors = vec!["Alice".to_string()];

        let result = effective_tracked_authors(&authors, "alice");

        assert_eq!(result, vec!["Alice".to_string()]);
    }
}
