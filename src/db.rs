use chrono::{DateTime, Utc};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::str::FromStr;

use crate::models::{CiStatus, PullRequest, User};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct DatabaseRepository {
    pool: SqlitePool,
}

impl DatabaseRepository {
    pub async fn connect(database_path: &str) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::from_str(database_path)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(options).await?;
        Ok(Self { pool })
    }

    pub async fn apply_migrations(&self) -> anyhow::Result<()> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub async fn save_pr(&self, pr: &PullRequest) -> anyhow::Result<()> {
        let reviewers_json = serde_json::to_string(&pr.requested_reviewers)?;
        sqlx::query(
            r#"
            INSERT INTO pull_requests (
              number, title, repository, author, draft, created_at_unix,
              updated_at_unix, ci_status, last_comment_unix, last_commit_unix,
              last_ci_status_update_unix, last_acknowledged_unix, requested_reviewers
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(repository, number) DO UPDATE SET
              title = excluded.title,
              repository = excluded.repository,
              author = excluded.author,
              draft = excluded.draft,
              updated_at_unix = excluded.updated_at_unix,
              ci_status = excluded.ci_status,
              last_comment_unix = excluded.last_comment_unix,
              last_commit_unix = excluded.last_commit_unix,
              last_ci_status_update_unix = excluded.last_ci_status_update_unix,
              last_acknowledged_unix = excluded.last_acknowledged_unix,
              requested_reviewers = excluded.requested_reviewers
            "#,
        )
        .bind(pr.number)
        .bind(&pr.title)
        .bind(&pr.repository)
        .bind(&pr.author)
        .bind(pr.draft)
        .bind(pr.created_at.timestamp())
        .bind(pr.updated_at.timestamp())
        .bind(pr.ci_status.as_i64())
        .bind(pr.last_comment_at.timestamp())
        .bind(pr.last_commit_at.timestamp())
        .bind(pr.last_ci_status_update_at.timestamp())
        .bind(pr.last_acknowledged_at.map(|t| t.timestamp()))
        .bind(reviewers_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_pr(&self, repo_name: &str, pr_number: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM pull_requests WHERE repository = ?1 AND number = ?2")
            .bind(repo_name)
            .bind(pr_number)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_prs_by_repository(&self, repo_name: &str) -> anyhow::Result<Vec<PullRequest>> {
        let rows = sqlx::query_as::<_, PullRequestRow>(
            r#"
            SELECT number, title, repository, author, draft, created_at_unix,
                   updated_at_unix, ci_status, last_comment_unix, last_commit_unix,
                   last_ci_status_update_unix, last_acknowledged_unix, requested_reviewers
            FROM pull_requests
            WHERE repository = ?1
            "#,
        )
        .bind(repo_name)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(PullRequestRow::to_model).collect()
    }

    pub async fn get_all_prs(&self) -> anyhow::Result<Vec<PullRequest>> {
        let rows = sqlx::query_as::<_, PullRequestRow>(
            r#"
            SELECT number, title, repository, author, draft, created_at_unix,
                   updated_at_unix, ci_status, last_comment_unix, last_commit_unix,
                   last_ci_status_update_unix, last_acknowledged_unix, requested_reviewers
            FROM pull_requests
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(PullRequestRow::to_model).collect()
    }

    pub async fn get_user(&self) -> anyhow::Result<Option<User>> {
        let rows = sqlx::query("SELECT id, username, access_token FROM users")
            .fetch_all(&self.pool)
            .await?;

        if rows.len() > 1 {
            anyhow::bail!("fatal error: expected at most 1 user, got {}", rows.len());
        }

        Ok(rows.into_iter().next().map(|row| User {
            username: row.get::<String, _>("username"),
            access_token: row.get::<String, _>("access_token"),
        }))
    }

    pub async fn save_user(&self, user: &User) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO users (username, access_token) VALUES (?1, ?2)")
            .bind(&user.username)
            .bind(&user.access_token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_pr(
        &self,
        repo_name: &str,
        pr_number: i64,
    ) -> anyhow::Result<Option<PullRequest>> {
        let row = sqlx::query_as::<_, PullRequestRow>(
            r#"
            SELECT number, title, repository, author, draft, created_at_unix,
                   updated_at_unix, ci_status, last_comment_unix, last_commit_unix,
                   last_ci_status_update_unix, last_acknowledged_unix, requested_reviewers
            FROM pull_requests
            WHERE repository = ?1 AND number = ?2
            LIMIT 1
            "#,
        )
        .bind(repo_name)
        .bind(pr_number)
        .fetch_optional(&self.pool)
        .await?;

        row.map(PullRequestRow::to_model).transpose()
    }

    pub async fn get_tracked_authors(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT author FROM tracked_authors")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("author"))
            .collect())
    }

    pub async fn save_tracked_author(&self, author: &str) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO tracked_authors (author) VALUES (?1)")
            .bind(author)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_tracked_author(&self, author: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tracked_authors WHERE author = ?1")
            .bind(author)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_tracked_repositories(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT repository FROM tracked_repositories")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("repository"))
            .collect())
    }

    pub async fn save_tracked_repository(&self, repo: &str) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO tracked_repositories (repository) VALUES (?1)")
            .bind(repo)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_tracked_repository(&self, repo: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tracked_repositories WHERE repository = ?1")
            .bind(repo)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[derive(Debug, FromRow)]
struct PullRequestRow {
    number: i64,
    title: String,
    repository: String,
    author: String,
    draft: bool,
    created_at_unix: i64,
    updated_at_unix: i64,
    ci_status: i64,
    last_comment_unix: i64,
    last_commit_unix: i64,
    last_ci_status_update_unix: i64,
    last_acknowledged_unix: Option<i64>,
    requested_reviewers: String,
}

impl PullRequestRow {
    fn to_model(self) -> anyhow::Result<PullRequest> {
        let requested_reviewers: Vec<String> = serde_json::from_str(&self.requested_reviewers)
            .map_err(|err| anyhow::anyhow!("unmarshal requested_reviewers: {err}"))?;

        Ok(PullRequest {
            number: self.number,
            title: self.title,
            repository: self.repository,
            author: self.author,
            draft: self.draft,
            created_at: unix_to_datetime(self.created_at_unix)?,
            updated_at: unix_to_datetime(self.updated_at_unix)?,
            ci_status: CiStatus::from_i64(self.ci_status),
            last_comment_at: unix_to_datetime(self.last_comment_unix)?,
            last_commit_at: unix_to_datetime(self.last_commit_unix)?,
            last_ci_status_update_at: unix_to_datetime(self.last_ci_status_update_unix)?,
            last_acknowledged_at: self
                .last_acknowledged_unix
                .map(unix_to_datetime)
                .transpose()?,
            requested_reviewers,
        })
    }
}

fn unix_to_datetime(seconds: i64) -> anyhow::Result<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid unix timestamp: {seconds}"))
}
