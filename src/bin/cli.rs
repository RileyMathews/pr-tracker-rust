use clap::{Parser, Subcommand};
use pr_tracker_rust::db::DatabaseRepository;
use pr_tracker_rust::github::GitHubClient;
use pr_tracker_rust::models::User;
use pr_tracker_rust::sync::{SyncRunSummary, sync_all_tracked};

#[derive(Debug, Parser)]
#[command(name = "pr-tracker-cli")]
#[command(about = "Track pull requests across repositories")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Auth {
        token: String,
    },
    Authors {
        #[command(subcommand)]
        command: AuthorCommand,
    },
    Repositories {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    Sync,
    Prs,
}

#[derive(Debug, Subcommand)]
enum AuthorCommand {
    List,
    Add { login: String },
    Remove { login: String },
}

#[derive(Debug, Subcommand)]
enum RepositoryCommand {
    List,
    Add { repository: String },
    Remove { repository: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let repo = open_repository().await?;

    match cli.command {
        Command::Auth { token } => handle_auth(&repo, &token).await?,
        Command::Authors { command } => handle_authors(&repo, command).await?,
        Command::Repositories { command } => handle_repositories(&repo, command).await?,
        Command::Sync => handle_sync(&repo).await?,
        Command::Prs => handle_prs(&repo).await?,
    }

    Ok(())
}

async fn open_repository() -> anyhow::Result<DatabaseRepository> {
    let db_path =
        std::env::var("PR_TRACKER_DB").unwrap_or_else(|_| "sqlite://./db.sqlite3".to_string());
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;
    Ok(repo)
}

async fn handle_auth(repo: &DatabaseRepository, token: &str) -> anyhow::Result<()> {
    if let Some(user) = repo.get_user().await? {
        anyhow::bail!(
            "a user is already authenticated as '{}', remove existing user first",
            user.username
        );
    }

    let github = GitHubClient::new(token.to_string())?;
    let user = github.fetch_authenticated_user().await?;

    let internal_user = User {
        username: user.login,
        access_token: token.to_string(),
    };
    repo.save_user(&internal_user).await?;
    println!("Authenticated as: {}", internal_user.username);

    Ok(())
}

async fn handle_authors(repo: &DatabaseRepository, command: AuthorCommand) -> anyhow::Result<()> {
    match command {
        AuthorCommand::List => {
            let authors = repo.get_tracked_authors().await?;
            println!("Authors:");
            for author in authors {
                println!("- {}", author);
            }
        }
        AuthorCommand::Add { login } => {
            repo.save_tracked_author(&login).await?;
            println!("Author '{}' added successfully", login);
        }
        AuthorCommand::Remove { login } => {
            repo.delete_tracked_author(&login).await?;
            println!("Author '{}' removed successfully", login);
        }
    }

    Ok(())
}

async fn handle_repositories(
    repo: &DatabaseRepository,
    command: RepositoryCommand,
) -> anyhow::Result<()> {
    match command {
        RepositoryCommand::List => {
            let repositories = repo.get_tracked_repositories().await?;
            println!("Repositories:");
            for repository in repositories {
                println!("- {}", repository);
            }
        }
        RepositoryCommand::Add { repository } => {
            repo.save_tracked_repository(&repository).await?;
            println!("Repository '{}' added successfully", repository);
        }
        RepositoryCommand::Remove { repository } => {
            repo.delete_tracked_repository(&repository).await?;
            println!("Repository '{}' removed successfully", repository);
        }
    }

    Ok(())
}

async fn handle_sync(repo: &DatabaseRepository) -> anyhow::Result<()> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'cli auth <token>' first")
    })?;

    let tracked_repositories = repo.get_tracked_repositories().await?;
    if tracked_repositories.is_empty() {
        println!("No repositories to sync");
        return Ok(());
    }

    let tracked_authors = repo.get_tracked_authors().await?;
    if tracked_authors.is_empty() {
        println!("No authors to sync");
        return Ok(());
    }

    let github = GitHubClient::new(user.access_token)?;
    let summary = sync_all_tracked(repo, &github).await?;

    let _ = notify_sync_changes(&summary);

    println!(
        "Sync complete: repos={} new={} updated={} deleted={}",
        summary.synced_repositories, summary.new_prs, summary.updated_prs, summary.deleted_prs
    );
    Ok(())
}

async fn handle_prs(repo: &DatabaseRepository) -> anyhow::Result<()> {
    let prs = repo.get_all_prs().await?;
    println!("PRs:");
    for pr in prs {
        println!(
            "- #{}: {} (Repository: {}, Author: {})",
            pr.number, pr.title, pr.repository, pr.author
        );
    }

    Ok(())
}

fn notify_sync_changes(summary: &SyncRunSummary) -> anyhow::Result<()> {
    let changed = summary.new_prs + summary.updated_prs;

    if changed == 0 {
        return Ok(())
    }

    let body = format!(
        "{} new, {} updated PRs",
        summary.new_prs, summary.updated_prs
    );

    #[cfg(target_os = "linux")]
    {
        notify_rust::Notification::new()
            .summary("PR Tracker")
            .body(&body)
            .appname("pr-tracker")
            .show()?;
    }

    Ok(())
}
