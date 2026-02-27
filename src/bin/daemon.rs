use std::time::Duration;

use pr_tracker_rust::db::DatabaseRepository;
use pr_tracker_rust::github::GitHubClient;
use pr_tracker_rust::sync::sync_all_tracked;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let interval_seconds: u64 = std::env::var("PR_TRACKER_SYNC_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60);

    let db_path = pr_tracker_rust::default_db_path();
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;

    let user = repo
        .get_user()
        .await?
        .ok_or_else(|| anyhow::anyhow!("no authenticated user found, run cli auth first"))?;
    let github = GitHubClient::new(user.access_token)?;

    loop {
        match sync_all_tracked(&repo, &github).await {
            Ok(summary) => {
                println!(
                    "sync ok repos={} new={} updated={} deleted={}",
                    summary.synced_repositories,
                    summary.new_prs,
                    summary.updated_prs,
                    summary.deleted_prs
                );
            }
            Err(err) => eprintln!("sync failed: {err:#}"),
        }

        tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
    }
}
