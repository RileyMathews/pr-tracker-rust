use tokio::sync::mpsc;

use crate::core::classify_team_members;
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
    let teams = github.fetch_user_teams().await?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_members: Vec<String> = Vec::new();
    for team in &teams {
        let members = github
            .fetch_team_members(&team.organization.login, &team.slug)
            .await?;
        for member in members {
            if seen.insert(member.login.to_lowercase()) {
                all_members.push(member.login);
            }
        }
    }

    let classified = classify_team_members(all_members, &user.username, &tracked_authors);

    Ok(TeamsPayload {
        tracked: classified.already_tracked,
        untracked: classified.candidates,
    })
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
