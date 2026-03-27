use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::core::{
    count_update_reasons, partition_updated_pull_requests, process_pull_request_sync_results,
    SyncDiff,
};
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
    pub updated_data_prs: Vec<PullRequest>,
    pub updated_attention_prs: Vec<PullRequest>,
    pub updated_reason_counts: BTreeMap<String, usize>,
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
        updated_data_prs: usize,
        updated_attention_prs: usize,
        updated_reason_counts: BTreeMap<String, usize>,
        deleted_prs: usize,
    },
}

pub fn format_sync_progress(progress: &SyncProgress) -> Option<String> {
    match progress {
        SyncProgress::FullSyncStarted { total_repositories } => Some(format!(
            "[sync] starting sync for {total_repositories} repositor{}",
            if *total_repositories == 1 { "y" } else { "ies" }
        )),
        SyncProgress::FullSyncRepositoryStarted { repository, .. } => {
            Some(format!("[sync] syncing repository: {repository}"))
        }
        SyncProgress::FullSyncRepositoryCompleted {
            repository,
            new_prs,
            updated_data_prs,
            updated_attention_prs,
            updated_reason_counts,
            deleted_prs,
            ..
        } => Some(format!(
            "[sync] repository complete: {repository} new={} updated_data={} updated_attention={} deleted={} reasons={:?}",
            new_prs, updated_data_prs, updated_attention_prs, deleted_prs, updated_reason_counts
        )),
    }
}

pub fn format_sync_summary(summary: &SyncRunSummary) -> String {
    format!(
        "Sync complete: repos={} new={} updated_data={} updated_attention={} deleted={} reasons={:?}",
        summary.synced_repositories,
        summary.new_prs.len(),
        summary.updated_data_prs.len(),
        summary.updated_attention_prs.len(),
        summary.deleted_prs.len(),
        summary.updated_reason_counts
    )
}

struct RepoSyncResult {
    repo_name: String,
    repo_index: usize,
    new_prs: Vec<PullRequest>,
    updated_data_prs: Vec<PullRequest>,
    updated_attention_prs: Vec<PullRequest>,
    updated_reason_counts: BTreeMap<String, usize>,
    deleted_prs: Vec<PullRequest>,
}

fn merge_reason_counts(target: &mut BTreeMap<String, usize>, source: BTreeMap<String, usize>) {
    for (reason, count) in source {
        *target.entry(reason).or_insert(0) += count;
    }
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
            updated_data_prs: repo_result.updated_data_prs.len(),
            updated_attention_prs: repo_result.updated_attention_prs.len(),
            updated_reason_counts: repo_result.updated_reason_counts.clone(),
            deleted_prs: repo_result.deleted_prs.len(),
        });
        summary.synced_repositories += 1;
        summary.new_prs.extend(repo_result.new_prs);
        summary
            .updated_data_prs
            .extend(repo_result.updated_data_prs);
        summary
            .updated_attention_prs
            .extend(repo_result.updated_attention_prs);
        merge_reason_counts(
            &mut summary.updated_reason_counts,
            repo_result.updated_reason_counts,
        );
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

    // Step 2: Fetch tracked PRs updated since the cutoff and refresh known open PRs.
    let existing_prs = repository.get_prs_by_repository(repo_name).await?;
    let tracked_pr_numbers: Vec<i64> = existing_prs.iter().map(|pr| pr.number).collect();
    let (discovery_sync_data, refresh_sync_data) = tokio::try_join!(
        service::fetch_tracked_pull_requests_for_sync(
            github,
            repo_name,
            tracked_authors,
            discovery_cutoff,
            username,
        ),
        service::refresh_tracked_pull_requests_for_sync(
            github,
            repo_name,
            &tracked_pr_numbers,
            username
        ),
    )?;
    let service::TrackedPullRequestSyncData {
        open_prs: fresh_prs,
        all_comments,
        closed_pr_numbers,
        max_updated_at,
    } = service::merge_tracked_pull_request_sync_data(discovery_sync_data, refresh_sync_data);

    // Step 3: Diff & persist.
    let SyncDiff {
        new_prs,
        updated_prs,
        removed_prs: _,
    } = process_pull_request_sync_results(&existing_prs, &fresh_prs, Utc::now());

    let updated_reason_counts = count_update_reasons(&updated_prs);
    let (updated_data_prs, updated_attention_prs) = partition_updated_pull_requests(updated_prs);

    for pr in &new_prs {
        repository.save_pr(pr).await?;
    }
    for pr in &updated_data_prs {
        repository.save_pr(pr).await?;
    }

    // Delete closed/merged PRs reported by GitHub search.
    for pr_number in &closed_pr_numbers {
        repository.delete_pr(repo_name, *pr_number).await?;
    }

    // Persist comments for all open PRs returned by the search query.
    for comment in all_comments {
        repository.save_comment(&comment).await?;
    }

    // Step 4: Update last_synced_at using the GitHub-side watermark.
    if let Some(max_ts) = max_updated_at {
        let watermark = max_ts - chrono::Duration::seconds(1);
        repository
            .update_tracked_repository_last_synced_at(repo_name, watermark)
            .await?;
    }

    // Step 5: Build result.
    let closed_set: HashSet<i64> = closed_pr_numbers.iter().copied().collect();
    let deleted_prs: Vec<PullRequest> = existing_prs
        .into_iter()
        .filter(|pr| closed_set.contains(&pr.number))
        .collect();

    Ok(RepoSyncResult {
        repo_name: repo_name.clone(),
        repo_index,
        new_prs,
        updated_data_prs,
        updated_attention_prs,
        updated_reason_counts,
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
