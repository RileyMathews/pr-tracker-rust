use chrono::{DateTime, Utc};

use crate::core::{process_pull_request_sync_results, SyncDiff};
use crate::db::DatabaseRepository;
use crate::github::GitHubClient;
use crate::models::PullRequest;
use crate::service;

const DEFAULT_MAX_PR_AGE_DAYS: i64 = 7;

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

pub async fn sync_all_tracked(
    repository: &DatabaseRepository,
    github: &GitHubClient,
) -> anyhow::Result<SyncRunSummary> {
    sync_all_tracked_with_progress(repository, github, |_| {}).await
}

pub async fn sync_all_tracked_with_progress<F>(
    repository: &DatabaseRepository,
    github: &GitHubClient,
    mut progress_callback: F,
) -> anyhow::Result<SyncRunSummary>
where
    F: FnMut(SyncProgress),
{
    let repositories = repository.get_tracked_repositories().await?;
    let tracked_authors = repository.get_tracked_authors().await?;
    let cutoff = pr_age_cutoff();

    let mut summary = SyncRunSummary::default();
    progress_callback(SyncProgress::FullSyncStarted {
        total_repositories: repositories.len(),
    });

    if repositories.is_empty() || tracked_authors.is_empty() {
        return Ok(summary);
    }

    let total_repositories = repositories.len();
    for (index, repo_name) in repositories.into_iter().enumerate() {
        progress_callback(SyncProgress::FullSyncRepositoryStarted {
            repository: repo_name.clone(),
            repository_index: index + 1,
            total_repositories,
        });
        let fresh_prs =
            service::fetch_tracked_pull_requests(github, &repo_name, &tracked_authors, cutoff)
                .await?;
        let existing_prs = repository.get_prs_by_repository(&repo_name).await?;

        // When using a cutoff, only consider recently-updated existing PRs as
        // candidates for removal. PRs older than the cutoff were never fetched,
        // so their absence doesn't mean they were closed.
        let existing_prs_for_diff = if let Some(cutoff) = cutoff {
            existing_prs
                .iter()
                .filter(|pr| pr.updated_at >= cutoff)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            existing_prs
        };

        let SyncDiff {
            new_prs,
            updated_prs,
            removed_prs,
        } = process_pull_request_sync_results(&existing_prs_for_diff, &fresh_prs, Utc::now());

        for pr in &new_prs {
            repository.save_pr(pr).await?;
        }
        for pr in &updated_prs {
            repository.save_pr(pr).await?;
        }
        for pr in &removed_prs {
            repository.delete_pr(&pr.repository, pr.number).await?;
        }

        let new_count = new_prs.len();
        let updated_count = updated_prs.len();
        let deleted_count = removed_prs.len();

        summary.synced_repositories += 1;
        summary.new_prs.extend(new_prs);
        summary.updated_prs.extend(updated_prs);
        summary.deleted_prs.extend(removed_prs);
        progress_callback(SyncProgress::FullSyncRepositoryCompleted {
            repository: repo_name.clone(),
            repository_index: index + 1,
            total_repositories,
            new_prs: new_count,
            updated_prs: updated_count,
            deleted_prs: deleted_count,
        });
    }

    Ok(summary)
}
