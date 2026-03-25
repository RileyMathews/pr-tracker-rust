use std::fmt::Display;

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;

pub mod graphql;
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
        let url =
            format!("{BASE_URL}/orgs/{org}/teams/{team_slug}/members?per_page={PER_PAGE}&page=1");
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

    pub async fn fetch_tracked_pull_requests_search(
        &self,
        repo_name: &str,
        authors: &[String],
        updated_after: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<graphql::PullRequestNode>> {
        ensure_not_blank("repo name", repo_name)?;
        if authors.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_nodes = Vec::new();
        let mut cursor: Option<String> = None;
        let updated_after =
            updated_after.map(|cutoff| cutoff.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        let search_query = graphql::build_tracked_pull_requests_search_query(
            repo_name,
            authors,
            updated_after.as_deref(),
        );

        loop {
            let variables = serde_json::json!({
                "query": search_query,
                "cursor": cursor,
            });

            let response: graphql::TrackedPullRequestSearchResponse = self
                .post_graphql(graphql::TRACKED_PULL_REQUESTS_SEARCH_QUERY, variables)
                .await?;

            let search = response.search;
            all_nodes.extend(search.nodes);

            if search.page_info.has_next_page {
                cursor = search.page_info.end_cursor;
                if cursor.is_none() {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(all_nodes)
    }

    async fn post_graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = "https://api.github.com/graphql";

        if self.log_requests {
            eprintln!("[github] POST {url}");
        }

        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let response = self.http.post(url).json(&body).send().await?;
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "github API request failed: status={} body={}",
                status.as_u16(),
                body.trim()
            );
        }

        let response_body: serde_json::Value = response.json().await?;

        if let Some(errors) = response_body.get("errors") {
            let data = response_body.get("data");
            let data_is_present = data.is_some_and(|d| !d.is_null());

            if !data_is_present {
                anyhow::bail!("graphql errors: {errors}");
            }

            if self.log_requests {
                eprintln!("[github] graphql response contained errors: {errors}");
            }
        }

        let data = response_body
            .get("data")
            .ok_or_else(|| anyhow::anyhow!("graphql response missing 'data' field"))?
            .clone();

        serde_json::from_value(data)
            .map_err(|err| anyhow::anyhow!("error decoding graphql response: {err}"))
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

#[cfg(test)]
mod tests {
    use super::parse_next_url;

    #[test]
    fn parse_next_url_extracts_next_link() {
        let header = concat!(
            "<https://api.github.com/resource?page=2>; rel=\"next\", ",
            "<https://api.github.com/resource?page=5>; rel=\"last\""
        );

        assert_eq!(
            parse_next_url(header),
            Some("https://api.github.com/resource?page=2".to_string())
        );
    }

    #[test]
    fn parse_next_url_returns_none_without_next_link() {
        let header = "<https://api.github.com/resource?page=5>; rel=\"last\"";

        assert_eq!(parse_next_url(header), None);
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
