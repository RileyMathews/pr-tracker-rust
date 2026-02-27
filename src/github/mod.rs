use std::fmt::Display;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;

pub mod schema;

const BASE_URL: &str = "https://api.github.com";
const PER_PAGE: i64 = 100;

#[derive(Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
    auth_token: String,
    log_requests: bool,
}

impl GitHubClient {
    pub fn new(auth_token: impl Into<String>) -> anyhow::Result<Self> {
        let auth_token = auth_token.into();
        if auth_token.trim().is_empty() {
            anyhow::bail!("auth token is required");
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("pr-tracker-rust"));
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {auth_token}"))?,
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self {
            http,
            auth_token,
            log_requests: false,
        })
    }

    pub fn with_request_logging(mut self, log_requests: bool) -> Self {
        self.log_requests = log_requests;
        self
    }

    pub async fn fetch_authenticated_user(&self) -> anyhow::Result<schema::User> {
        self.get_json(&format!("{BASE_URL}/user")).await
    }

    pub async fn fetch_open_pull_requests(
        &self,
        repo_name: &str,
    ) -> anyhow::Result<Vec<schema::PullRequest>> {
        ensure_not_blank("repo name", repo_name)?;

        self.get_paginated(&format!(
            "{BASE_URL}/repos/{repo_name}/pulls?state=open&per_page={PER_PAGE}&page=1"
        ))
        .await
    }

    pub async fn fetch_pull_request_details(
        &self,
        repo_name: &str,
        pr_id: i64,
    ) -> anyhow::Result<schema::PullRequestDetails> {
        ensure_not_blank("repo name", repo_name)?;
        if pr_id <= 0 {
            anyhow::bail!("pr id must be greater than zero");
        }

        let mut details: schema::PullRequestDetails = self
            .get_json(&format!("{BASE_URL}/repos/{repo_name}/pulls/{pr_id}"))
            .await?;

        details.issue_comments = self
            .get_paginated(&format!(
                "{BASE_URL}/repos/{repo_name}/issues/{pr_id}/comments?per_page={PER_PAGE}&page=1"
            ))
            .await?;

        details.review_comments = self
            .get_paginated(&format!(
                "{BASE_URL}/repos/{repo_name}/pulls/{pr_id}/comments?per_page={PER_PAGE}&page=1"
            ))
            .await?;

        Ok(details)
    }

    pub async fn fetch_pull_request_ci_statuses(
        &self,
        repo_name: &str,
        pr_id: i64,
    ) -> anyhow::Result<schema::PullRequestCiStatuses> {
        ensure_not_blank("repo name", repo_name)?;
        if pr_id <= 0 {
            anyhow::bail!("pr id must be greater than zero");
        }

        let pr: schema::PullRequestHead = self
            .get_json(&format!("{BASE_URL}/repos/{repo_name}/pulls/{pr_id}"))
            .await?;

        if pr.head.sha.trim().is_empty() {
            anyhow::bail!("pull request head sha is missing");
        }

        let status_response: schema::CombinedStatus = self
            .get_json(&format!(
                "{BASE_URL}/repos/{repo_name}/commits/{}/status",
                pr.head.sha
            ))
            .await?;

        let mut check_runs = Vec::new();
        let mut next_url = Some(format!(
            "{BASE_URL}/repos/{repo_name}/commits/{}/check-runs?per_page={PER_PAGE}&page=1",
            pr.head.sha
        ));

        while let Some(url) = next_url {
            let (page, link_header): (schema::CheckRunsPage, Option<String>) =
                self.get_json_with_link(&url).await?;
            check_runs.extend(page.check_runs);
            next_url = link_header.and_then(|link| parse_next_url(&link));
        }

        Ok(schema::PullRequestCiStatuses {
            pull_request_number: pr.number,
            head_sha: pr.head.sha,
            combined_state: status_response.state,
            statuses: status_response.statuses,
            check_runs,
        })
    }

    pub async fn fetch_user_teams(&self) -> anyhow::Result<Vec<schema::UserTeam>> {
        let url = format!("{BASE_URL}/user/teams?per_page={PER_PAGE}&page=1");
        self.get_paginated(&url).await.map_err(|err| {
            if err.to_string().contains("status=403") {
                anyhow::anyhow!(
                    "failed to fetch GitHub teams: token likely lacks 'read:org' scope. \
                     Regenerate your token with 'read:org' permission and run 'prt auth' again.\n\
                     Original error: {err}"
                )
            } else {
                err
            }
        })
    }

    pub async fn fetch_team_members(
        &self,
        org: &str,
        team_slug: &str,
    ) -> anyhow::Result<Vec<schema::TeamMember>> {
        let url = format!(
            "{BASE_URL}/orgs/{org}/teams/{team_slug}/members?per_page={PER_PAGE}&page=1"
        );
        self.get_paginated(&url).await.map_err(|err| {
            if err.to_string().contains("status=403") {
                anyhow::anyhow!(
                    "failed to fetch members for team '{team_slug}' in org '{org}': \
                     token likely lacks 'read:org' scope.\n\
                     Original error: {err}"
                )
            } else {
                err
            }
        })
    }

    async fn get_paginated<T>(&self, first_url: &str) -> anyhow::Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut next_url = Some(first_url.to_string());
        let mut items = Vec::new();

        while let Some(url) = next_url {
            let (page_items, link_header): (Vec<T>, Option<String>) =
                self.get_json_with_link(&url).await?;
            items.extend(page_items);
            next_url = link_header.and_then(|link| parse_next_url(&link));
        }

        Ok(items)
    }

    async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
    {
        let (value, _): (T, Option<String>) = self.get_json_with_link(url).await?;
        Ok(value)
    }

    async fn get_json_with_link<T>(&self, url: &str) -> anyhow::Result<(T, Option<String>)>
    where
        T: DeserializeOwned,
    {
        if self.log_requests {
            eprintln!("[github] GET {url}");
        }
        let response = self.http.get(url).send().await?;
        let status = response.status();
        let link_header = response
            .headers()
            .get("Link")
            .and_then(|h| h.to_str().ok())
            .map(|v| v.to_string());

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "github API request failed: status={} body={}",
                status.as_u16(),
                body.trim()
            );
        }

        let body = response.text().await?;
        let value = serde_json::from_str::<T>(&body)
            .map_err(|err| anyhow::anyhow!("error decoding response body for {url}: {err}"))?;
        Ok((value, link_header))
    }

    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }
}

fn ensure_not_blank(label: impl Display, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{} is required", label);
    }
    Ok(())
}

pub fn parse_next_url(link_header: &str) -> Option<String> {
    link_header
        .split(',')
        .map(str::trim)
        .find(|segment| segment.contains("rel=\"next\""))
        .and_then(|segment| {
            let start = segment.find('<')?;
            let end = segment.find('>')?;
            if end <= start + 1 {
                return None;
            }
            Some(segment[start + 1..end].to_string())
        })
}
