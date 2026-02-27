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

pub async fn sync_all_tracked(
    repository: &DatabaseRepository,
    github: &GitHubClient,
) -> anyhow::Result<SyncRunSummary> {
    let repositories = repository.get_tracked_repositories().await?;
    let tracked_authors = repository.get_tracked_authors().await?;

    let mut summary = SyncRunSummary::default();
    if repositories.is_empty() || tracked_authors.is_empty() {
        return Ok(summary);
    }

    for repo_name in repositories {
        eprintln!("[sync] syncing repository: {repo_name}");
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
        eprintln!(
            "[sync] repository complete: {repo_name} new={} updated={} deleted={}",
            new_prs.len(),
            updated_prs.len(),
            removed_prs.len()
        );
    }

    Ok(summary)
}
