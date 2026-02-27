use chrono::Utc;

use crate::core::{process_pull_request_sync_results, SyncDiff};
use crate::db::DatabaseRepository;
use crate::github::GitHubClient;
use crate::service;

#[derive(Debug, Default)]
pub struct SyncRunSummary {
    pub synced_repositories: usize,
    pub new_prs: usize,
    pub updated_prs: usize,
    pub deleted_prs: usize,
}

#[derive(Debug, Default)]
pub struct QuickRefreshSummary {
    pub total_prs: usize,
    pub refreshed_prs: usize,
    pub failed_prs: usize,
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
    QuickRefreshStarted {
        total_prs: usize,
    },
    QuickRefreshPullRequestStarted {
        repository: String,
        pr_number: i64,
        pr_index: usize,
        total_prs: usize,
    },
    QuickRefreshPullRequestCompleted {
        repository: String,
        pr_number: i64,
        pr_index: usize,
        total_prs: usize,
        ok: bool,
        error: Option<String>,
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
            service::fetch_tracked_pull_requests(github, &repo_name, &tracked_authors).await?;
        let existing_prs = repository.get_prs_by_repository(&repo_name).await?;

        let SyncDiff {
            new_prs,
            updated_prs,
            removed_prs,
        } = process_pull_request_sync_results(&existing_prs, &fresh_prs, Utc::now());

        for pr in &new_prs {
            repository.save_pr(pr).await?;
        }
        for pr in &updated_prs {
            repository.save_pr(pr).await?;
        }
        for pr in &removed_prs {
            repository.delete_pr(&pr.repository, pr.number).await?;
        }

        summary.synced_repositories += 1;
        summary.new_prs += new_prs.len();
        summary.updated_prs += updated_prs.len();
        summary.deleted_prs += removed_prs.len();
        progress_callback(SyncProgress::FullSyncRepositoryCompleted {
            repository: repo_name.clone(),
            repository_index: index + 1,
            total_repositories,
            new_prs: new_prs.len(),
            updated_prs: updated_prs.len(),
            deleted_prs: removed_prs.len(),
        });
    }

    Ok(summary)
}

pub async fn refresh_existing_pull_requests(
    repository: &DatabaseRepository,
    github: &GitHubClient,
) -> anyhow::Result<QuickRefreshSummary> {
    refresh_existing_pull_requests_with_progress(repository, github, |_| {}).await
}

pub async fn refresh_existing_pull_requests_with_progress<F>(
    repository: &DatabaseRepository,
    github: &GitHubClient,
    mut progress_callback: F,
) -> anyhow::Result<QuickRefreshSummary>
where
    F: FnMut(SyncProgress),
{
    let existing_prs = repository.get_all_prs().await?;
    let mut summary = QuickRefreshSummary {
        total_prs: existing_prs.len(),
        ..QuickRefreshSummary::default()
    };

    progress_callback(SyncProgress::QuickRefreshStarted {
        total_prs: summary.total_prs,
    });

    for (index, existing_pr) in existing_prs.into_iter().enumerate() {
        progress_callback(SyncProgress::QuickRefreshPullRequestStarted {
            repository: existing_pr.repository.clone(),
            pr_number: existing_pr.number,
            pr_index: index + 1,
            total_prs: summary.total_prs,
        });

        let refreshed_result = service::fetch_pull_request_details(
            github,
            &existing_pr.repository,
            existing_pr.number,
        )
        .await;

        match refreshed_result {
            Ok(mut refreshed_pr) => {
                refreshed_pr.last_acknowledged_at = existing_pr.last_acknowledged_at;
                refreshed_pr.last_ci_status_update_at =
                    if existing_pr.ci_status != refreshed_pr.ci_status {
                        Utc::now()
                    } else {
                        existing_pr.last_ci_status_update_at
                    };

                repository.save_pr(&refreshed_pr).await?;
                summary.refreshed_prs += 1;

                progress_callback(SyncProgress::QuickRefreshPullRequestCompleted {
                    repository: refreshed_pr.repository,
                    pr_number: refreshed_pr.number,
                    pr_index: index + 1,
                    total_prs: summary.total_prs,
                    ok: true,
                    error: None,
                });
            }
            Err(err) => {
                summary.failed_prs += 1;
                let error = err.to_string();
                progress_callback(SyncProgress::QuickRefreshPullRequestCompleted {
                    repository: existing_pr.repository,
                    pr_number: existing_pr.number,
                    pr_index: index + 1,
                    total_prs: summary.total_prs,
                    ok: false,
                    error: Some(error),
                });
            }
        }
    }

    Ok(summary)
}
