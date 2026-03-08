use tokio::sync::mpsc;

use crate::db::DatabaseRepository;
use crate::github::GitHubClient;
use crate::sync::{sync_all_tracked_with_progress, SyncRunSummary};

/// Background job types that can be active.
#[derive(Clone, Copy)]
pub enum BackgroundJob {
    FullSync,
    TeamsFetch,
}

/// Messages sent from background tasks to the main loop.
pub enum BackgroundMessage {
    Progress,
    FullSyncFinished(anyhow::Result<SyncRunSummary>),
    TeamsFetchFinished(anyhow::Result<TeamsPayload>),
}

/// Payload returned from team fetch operations.
pub struct TeamsPayload {
    pub tracked: Vec<String>,
    pub untracked: Vec<String>,
}

/// Spawn a full sync job in the background.
pub fn spawn_full_sync(repo: DatabaseRepository, tx: mpsc::UnboundedSender<BackgroundMessage>) {
    tokio::spawn(async move {
        let progress_tx = tx.clone();
        let result = run_full_sync(repo, progress_tx).await;
        let _ = tx.send(BackgroundMessage::FullSyncFinished(result));
    });
}

/// Spawn a teams fetch job in the background.
pub fn spawn_teams_fetch(repo: DatabaseRepository, tx: mpsc::UnboundedSender<BackgroundMessage>) {
    tokio::spawn(async move {
        let result = run_teams_fetch(repo).await;
        let _ = tx.send(BackgroundMessage::TeamsFetchFinished(result));
    });
}

/// Run a full sync operation.
async fn run_full_sync(
    repo: DatabaseRepository,
    tx: mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<SyncRunSummary> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'cli auth <token>' first")
    })?;
    let username = user.username.clone();
    let github = GitHubClient::new(user.access_token)?;

    sync_all_tracked_with_progress(&repo, &github, &username, |_| {
        let _ = tx.send(BackgroundMessage::Progress);
    })
    .await
}

/// Run a teams fetch operation.
async fn run_teams_fetch(repo: DatabaseRepository) -> anyhow::Result<TeamsPayload> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'prt auth <token>' first")
    })?;
    let github = GitHubClient::new(user.access_token.clone())?;

    let tracked_authors = repo.get_tracked_authors().await?;
    let tracked_set: std::collections::HashSet<String> =
        tracked_authors.iter().map(|s| s.to_lowercase()).collect();
    let current_login_lower = user.username.to_lowercase();

    let teams = github.fetch_user_teams().await?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_members: Vec<String> = Vec::new();
    for team in &teams {
        let members = github
            .fetch_team_members(&team.organization.login, &team.slug)
            .await?;
        for member in members {
            let lower = member.login.to_lowercase();
            if lower != current_login_lower && seen.insert(lower.clone()) {
                all_members.push(member.login);
            }
        }
    }

    let mut untracked: Vec<String> = all_members
        .into_iter()
        .filter(|login| !tracked_set.contains(&login.to_lowercase()))
        .collect();
    untracked.sort();

    let mut tracked = tracked_authors;
    tracked.sort();

    Ok(TeamsPayload { tracked, untracked })
}

/// Get a human-readable label for a background job.
pub fn background_job_label(job: BackgroundJob) -> &'static str {
    match job {
        BackgroundJob::FullSync => "sync",
        BackgroundJob::TeamsFetch => "fetching teams",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_job_label_full_sync() {
        assert_eq!(background_job_label(BackgroundJob::FullSync), "sync");
    }

    #[test]
    fn background_job_label_teams_fetch() {
        assert_eq!(
            background_job_label(BackgroundJob::TeamsFetch),
            "fetching teams"
        );
    }
}
