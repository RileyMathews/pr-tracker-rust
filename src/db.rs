use chrono::{DateTime, Utc};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::fs;
use std::path::Path;
use std::str::FromStr;

use crate::models::{ApprovalStatus, CiStatus, PrComment, PullRequest, TrackedRepository, User};
use crate::pr_repository::{build_pr_dashboard, PrDashboard};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct DatabaseRepository {
    pool: SqlitePool,
}

impl DatabaseRepository {
    pub async fn connect(database_path: &str) -> anyhow::Result<Self> {
        ensure_database_parent_dir(database_path)?;

        let options = SqliteConnectOptions::from_str(database_path)?
            .create_if_missing(true)
            .pragma("foreign_keys", "ON");
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
              number, title, repository, author, head_sha, draft, created_at_unix,
              updated_at_unix, ci_status, last_comment_unix, last_commit_unix,
              last_ci_status_update_unix, last_acknowledged_unix, requested_reviewers,
              approval_status, last_review_status_update_unix, user_has_reviewed
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(repository, number) DO UPDATE SET
              title = excluded.title,
              repository = excluded.repository,
              author = excluded.author,
              head_sha = excluded.head_sha,
              draft = excluded.draft,
              updated_at_unix = excluded.updated_at_unix,
              ci_status = excluded.ci_status,
              last_comment_unix = excluded.last_comment_unix,
              last_commit_unix = excluded.last_commit_unix,
              last_ci_status_update_unix = excluded.last_ci_status_update_unix,
              last_acknowledged_unix = excluded.last_acknowledged_unix,
              requested_reviewers = excluded.requested_reviewers,
              approval_status = excluded.approval_status,
              last_review_status_update_unix = excluded.last_review_status_update_unix,
              user_has_reviewed = excluded.user_has_reviewed
            "#,
        )
        .bind(pr.number)
        .bind(&pr.title)
        .bind(&pr.repository)
        .bind(&pr.author)
        .bind(&pr.head_sha)
        .bind(pr.draft)
        .bind(pr.created_at.timestamp())
        .bind(pr.updated_at.timestamp())
        .bind(pr.ci_status.as_i64())
        .bind(pr.last_comment_at.timestamp())
        .bind(pr.last_commit_at.timestamp())
        .bind(pr.last_ci_status_update_at.timestamp())
        .bind(pr.last_acknowledged_at.map(|t| t.timestamp()))
        .bind(reviewers_json)
        .bind(pr.approval_status.as_i64())
        .bind(pr.last_review_status_update_at.timestamp())
        .bind(pr.user_has_reviewed)
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
        let rows = sqlx::query_as::<_, PullRequestWithCommentsRow>(
            r#"
            SELECT 
                pr.number,
                pr.title,
                pr.repository,
                pr.author,
                pr.head_sha,
                pr.draft,
                pr.created_at_unix,
                pr.updated_at_unix,
                pr.ci_status,
                pr.last_comment_unix,
                pr.last_commit_unix,
                pr.last_ci_status_update_unix,
                pr.last_acknowledged_unix,
                pr.requested_reviewers,
                pr.approval_status,
                pr.last_review_status_update_unix,
                pr.user_has_reviewed,
                COALESCE(
                    json_group_array(
                        json_object(
                            'id', c.id,
                            'repository', c.repository,
                            'pr_number', c.pr_number,
                            'author', c.author,
                            'body', c.body,
                            'created_at_unix', c.created_at_unix,
                            'updated_at_unix', c.updated_at_unix,
                            'is_review_comment', c.is_review_comment,
                            'review_state', c.review_state
                        )
                        ORDER BY c.created_at_unix ASC
                    ) FILTER (WHERE c.id IS NOT NULL),
                    '[]'
                ) as comments_json
            FROM pull_requests pr
            LEFT JOIN pr_comments c 
                ON pr.repository = c.repository AND pr.number = c.pr_number
            WHERE pr.repository = ?1
            GROUP BY pr.repository, pr.number
            ORDER BY pr.updated_at_unix DESC
            "#,
        )
        .bind(repo_name)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|row| row.into_model()).collect()
    }

    pub async fn get_all_prs(&self) -> anyhow::Result<Vec<PullRequest>> {
        // Delegate to the JOIN-based implementation
        self.get_all_prs_with_comments().await
    }

    pub async fn get_pr_dashboard(&self, username: &str) -> anyhow::Result<PrDashboard> {
        let prs = self.get_all_prs_with_comments().await?;
        Ok(build_pr_dashboard(prs, username))
    }

    pub async fn get_all_prs_with_comments(&self) -> anyhow::Result<Vec<PullRequest>> {
        let rows = sqlx::query_as::<_, PullRequestWithCommentsRow>(
            r#"
            SELECT 
                pr.number,
                pr.title,
                pr.repository,
                pr.author,
                pr.head_sha,
                pr.draft,
                pr.created_at_unix,
                pr.updated_at_unix,
                pr.ci_status,
                pr.last_comment_unix,
                pr.last_commit_unix,
                pr.last_ci_status_update_unix,
                pr.last_acknowledged_unix,
                pr.requested_reviewers,
                pr.approval_status,
                pr.last_review_status_update_unix,
                pr.user_has_reviewed,
                COALESCE(
                    json_group_array(
                        json_object(
                            'id', c.id,
                            'repository', c.repository,
                            'pr_number', c.pr_number,
                            'author', c.author,
                            'body', c.body,
                            'created_at_unix', c.created_at_unix,
                            'updated_at_unix', c.updated_at_unix,
                            'is_review_comment', c.is_review_comment,
                            'review_state', c.review_state
                        )
                        ORDER BY c.created_at_unix ASC
                    ) FILTER (WHERE c.id IS NOT NULL),
                    '[]'
                ) as comments_json
            FROM pull_requests pr
            LEFT JOIN pr_comments c 
                ON pr.repository = c.repository AND pr.number = c.pr_number
            GROUP BY pr.repository, pr.number
            ORDER BY pr.updated_at_unix DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|row| row.into_model()).collect()
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
        let row = sqlx::query_as::<_, PullRequestWithCommentsRow>(
            r#"
            SELECT 
                pr.number,
                pr.title,
                pr.repository,
                pr.author,
                pr.head_sha,
                pr.draft,
                pr.created_at_unix,
                pr.updated_at_unix,
                pr.ci_status,
                pr.last_comment_unix,
                pr.last_commit_unix,
                pr.last_ci_status_update_unix,
                pr.last_acknowledged_unix,
                pr.requested_reviewers,
                pr.approval_status,
                pr.last_review_status_update_unix,
                pr.user_has_reviewed,
                COALESCE(
                    json_group_array(
                        json_object(
                            'id', c.id,
                            'repository', c.repository,
                            'pr_number', c.pr_number,
                            'author', c.author,
                            'body', c.body,
                            'created_at_unix', c.created_at_unix,
                            'updated_at_unix', c.updated_at_unix,
                            'is_review_comment', c.is_review_comment,
                            'review_state', c.review_state
                        )
                        ORDER BY c.created_at_unix ASC
                    ) FILTER (WHERE c.id IS NOT NULL),
                    '[]'
                ) as comments_json
            FROM pull_requests pr
            LEFT JOIN pr_comments c 
                ON pr.repository = c.repository AND pr.number = c.pr_number
            WHERE pr.repository = ?1 AND pr.number = ?2
            GROUP BY pr.repository, pr.number
            "#,
        )
        .bind(repo_name)
        .bind(pr_number)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| r.into_model()).transpose()
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
        sqlx::query("INSERT OR IGNORE INTO tracked_authors (author) VALUES (?1)")
            .bind(author)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_tracked_authors_batch(&self, authors: &[String]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        for author in authors {
            sqlx::query("INSERT OR IGNORE INTO tracked_authors (author) VALUES (?1)")
                .bind(author)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_tracked_author(&self, author: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tracked_authors WHERE author = ?1")
            .bind(author)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_tracked_repositories(&self) -> anyhow::Result<Vec<TrackedRepository>> {
        let rows = sqlx::query("SELECT repository, last_synced_at_unix FROM tracked_repositories")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let repository: String = row.get("repository");
                let last_synced_at_unix: Option<i64> = row.get("last_synced_at_unix");
                TrackedRepository {
                    repository,
                    last_synced_at: last_synced_at_unix
                        .and_then(|ts| DateTime::from_timestamp(ts, 0)),
                }
            })
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

    pub async fn reset_all_tracked_repositories_last_synced_at(&self) -> anyhow::Result<usize> {
        let result = sqlx::query("UPDATE tracked_repositories SET last_synced_at_unix = NULL")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    pub async fn update_tracked_repository_last_synced_at(
        &self,
        repo: &str,
        last_synced_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE tracked_repositories SET last_synced_at_unix = ?1 WHERE repository = ?2",
        )
        .bind(last_synced_at.timestamp())
        .bind(repo)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_comment(&self, comment: &PrComment) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO pr_comments (
                id, repository, pr_number, author, body, created_at_unix,
                updated_at_unix, is_review_comment, review_state
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                repository = excluded.repository,
                pr_number = excluded.pr_number,
                author = excluded.author,
                body = excluded.body,
                created_at_unix = excluded.created_at_unix,
                updated_at_unix = excluded.updated_at_unix,
                is_review_comment = excluded.is_review_comment,
                review_state = excluded.review_state
            "#,
        )
        .bind(&comment.id)
        .bind(&comment.repository)
        .bind(comment.pr_number)
        .bind(&comment.author)
        .bind(&comment.body)
        .bind(comment.created_at.timestamp())
        .bind(comment.updated_at.timestamp())
        .bind(if comment.is_review_comment {
            1i64
        } else {
            0i64
        })
        .bind(&comment.review_state)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_comments_for_pr(
        &self,
        repository: &str,
        pr_number: i64,
    ) -> anyhow::Result<Vec<PrComment>> {
        let rows = sqlx::query_as::<_, PrCommentRow>(
            r#"
            SELECT id, repository, pr_number, author, body, created_at_unix,
                   updated_at_unix, is_review_comment, review_state
            FROM pr_comments
            WHERE repository = ?1 AND pr_number = ?2
            ORDER BY created_at_unix ASC
            "#,
        )
        .bind(repository)
        .bind(pr_number)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(PrCommentRow::into_model).collect()
    }

    pub async fn delete_comments_for_pr(
        &self,
        repository: &str,
        pr_number: i64,
    ) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM pr_comments WHERE repository = ?1 AND pr_number = ?2")
            .bind(repository)
            .bind(pr_number)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn ensure_database_parent_dir(database_path: &str) -> anyhow::Result<()> {
    let Some(path) = sqlite_file_path(database_path) else {
        return Ok(());
    };

    let Some(parent) = path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent)?;
    Ok(())
}

fn sqlite_file_path(database_path: &str) -> Option<&Path> {
    let path = database_path
        .strip_prefix("sqlite://")
        .or_else(|| database_path.strip_prefix("sqlite:"))
        .unwrap_or(database_path)
        .split('?')
        .next()
        .unwrap_or(database_path);

    if path == ":memory:" || path.is_empty() {
        return None;
    }

    Some(Path::new(path))
}

fn unix_to_datetime(seconds: i64) -> anyhow::Result<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid unix timestamp: {seconds}"))
}

#[derive(Debug, FromRow)]
struct PrCommentRow {
    id: String,
    repository: String,
    pr_number: i64,
    author: String,
    body: String,
    created_at_unix: i64,
    updated_at_unix: i64,
    is_review_comment: i64,
    review_state: Option<String>,
}

impl PrCommentRow {
    fn into_model(self) -> anyhow::Result<PrComment> {
        Ok(PrComment {
            id: self.id,
            repository: self.repository,
            pr_number: self.pr_number,
            author: self.author,
            body: self.body,
            created_at: unix_to_datetime(self.created_at_unix)?,
            updated_at: unix_to_datetime(self.updated_at_unix)?,
            is_review_comment: self.is_review_comment != 0,
            review_state: self.review_state,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct CommentJson {
    id: Option<String>,
    repository: Option<String>,
    pr_number: Option<i64>,
    author: Option<String>,
    body: Option<String>,
    created_at_unix: Option<i64>,
    updated_at_unix: Option<i64>,
    is_review_comment: Option<i64>, // stored as 0/1 in JSON
    review_state: Option<String>,
}

impl CommentJson {
    fn into_model(self) -> anyhow::Result<Option<PrComment>> {
        if self.id.is_none() {
            return Ok(None);
        }

        let id = self
            .id
            .ok_or_else(|| anyhow::anyhow!("missing comment field: id"))?;
        let repository = self
            .repository
            .ok_or_else(|| anyhow::anyhow!("missing comment field: repository"))?;
        let pr_number = self
            .pr_number
            .ok_or_else(|| anyhow::anyhow!("missing comment field: pr_number"))?;
        let author = self
            .author
            .ok_or_else(|| anyhow::anyhow!("missing comment field: author"))?;
        let body = self
            .body
            .ok_or_else(|| anyhow::anyhow!("missing comment field: body"))?;
        let created_at_unix = self
            .created_at_unix
            .ok_or_else(|| anyhow::anyhow!("missing comment field: created_at_unix"))?;
        let updated_at_unix = self
            .updated_at_unix
            .ok_or_else(|| anyhow::anyhow!("missing comment field: updated_at_unix"))?;
        let is_review_comment = self
            .is_review_comment
            .ok_or_else(|| anyhow::anyhow!("missing comment field: is_review_comment"))?;

        Ok(Some(PrComment {
            id,
            repository,
            pr_number,
            author,
            body,
            created_at: unix_to_datetime(created_at_unix)?,
            updated_at: unix_to_datetime(updated_at_unix)?,
            is_review_comment: is_review_comment != 0,
            review_state: self.review_state,
        }))
    }
}

#[derive(Debug, sqlx::FromRow)]
struct PullRequestWithCommentsRow {
    number: i64,
    title: String,
    repository: String,
    author: String,
    head_sha: String,
    draft: bool,
    created_at_unix: i64,
    updated_at_unix: i64,
    ci_status: i64,
    last_comment_unix: i64,
    last_commit_unix: i64,
    last_ci_status_update_unix: i64,
    last_acknowledged_unix: Option<i64>,
    requested_reviewers: String,
    approval_status: i64,
    last_review_status_update_unix: i64,
    user_has_reviewed: bool,
    comments_json: String,
}

impl PullRequestWithCommentsRow {
    fn into_model(self) -> anyhow::Result<PullRequest> {
        // First, deserialize requested_reviewers (same as PullRequestRow)
        let requested_reviewers: Vec<String> = serde_json::from_str(&self.requested_reviewers)
            .map_err(|err| anyhow::anyhow!("unmarshal requested_reviewers: {err}"))?;

        // Deserialize the JSON array of comments
        let comments: Vec<CommentJson> = serde_json::from_str(&self.comments_json)
            .map_err(|err| anyhow::anyhow!("unmarshal comments_json: {err}"))?;

        // Convert each CommentJson to PrComment
        let comments: Vec<PrComment> = comments
            .into_iter()
            .map(|c| c.into_model())
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        // Build and return the PullRequest (copy the pattern from existing PullRequestRow::into_model)
        Ok(PullRequest {
            number: self.number,
            title: self.title,
            repository: self.repository,
            author: self.author,
            head_sha: self.head_sha,
            draft: self.draft,
            created_at: unix_to_datetime(self.created_at_unix)?,
            updated_at: unix_to_datetime(self.updated_at_unix)?,
            ci_status: CiStatus::from_i64(self.ci_status),
            last_comment_at: unix_to_datetime(self.last_comment_unix)?,
            last_commit_at: unix_to_datetime(self.last_commit_unix)?,
            last_ci_status_update_at: unix_to_datetime(self.last_ci_status_update_unix)?,
            approval_status: ApprovalStatus::from_i64(self.approval_status),
            last_review_status_update_at: unix_to_datetime(self.last_review_status_update_unix)?,
            last_acknowledged_at: self
                .last_acknowledged_unix
                .map(unix_to_datetime)
                .transpose()?,
            requested_reviewers,
            user_has_reviewed: self.user_has_reviewed,
            comments, // NEW: populated from JSON
        })
    }
}

#[cfg(test)]
mod tests {
    use super::sqlite_file_path;

    #[test]
    fn extracts_relative_sqlite_file_path() {
        assert_eq!(
            sqlite_file_path("sqlite://./db.sqlite3"),
            Some(std::path::Path::new("./db.sqlite3"))
        );
    }

    #[test]
    fn extracts_absolute_sqlite_file_path() {
        assert_eq!(
            sqlite_file_path("sqlite:///tmp/pr-tracker/db.sqlite3?mode=rwc"),
            Some(std::path::Path::new("/tmp/pr-tracker/db.sqlite3"))
        );
    }

    #[test]
    fn ignores_in_memory_database() {
        assert_eq!(sqlite_file_path("sqlite::memory:"), None);
    }
}
