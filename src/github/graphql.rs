use serde::Deserialize;

pub const TRACKED_PULL_REQUESTS_SEARCH_QUERY: &str = r#"
query($query: String!, $cursor: String) {
  search(query: $query, type: ISSUE, first: 100, after: $cursor) {
    pageInfo {
      hasNextPage
      endCursor
    }
    nodes {
      ... on PullRequest {
        number
        title
        isDraft
        createdAt
        updatedAt
        state
        headRefOid
        author {
          login
        }
        reviewRequests(first: 100) {
          nodes {
            requestedReviewer {
              ... on User {
                login
              }
            }
          }
        }
        commits(last: 1) {
          nodes {
            commit {
              statusCheckRollup {
                state
              }
            }
          }
        }
        comments(last: 100) {
          nodes {
            id
            author { login }
            body
            createdAt
            updatedAt
          }
        }
        reviews(last: 100) {
          nodes {
            id
            author { login }
            body
            createdAt
            updatedAt
            state
            submittedAt
          }
        }
        latestReviews(first: 100) {
          nodes {
            state
            submittedAt
            author {
              login
            }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub struct TrackedPullRequestSearchResponse {
    pub search: TrackedPullRequestSearchResult,
}

#[derive(Debug, Deserialize)]
pub struct TrackedPullRequestSearchResult {
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    #[serde(default)]
    pub nodes: Vec<PullRequestNode>,
}

#[derive(Debug, Deserialize)]
pub struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestNode {
    pub number: i64,
    pub title: String,
    #[serde(rename = "isDraft")]
    pub is_draft: bool,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub state: String,
    pub author: Option<Author>,
    #[serde(rename = "reviewRequests")]
    pub review_requests: ReviewRequestConnection,
    #[serde(rename = "headRefOid")]
    pub head_ref_oid: String,
    pub commits: CommitConnection,
    pub comments: CommentConnection,
    pub reviews: ReviewConnection,
    #[serde(rename = "latestReviews")]
    pub latest_reviews: LatestReviewConnection,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewRequestConnection {
    #[serde(default)]
    pub nodes: Vec<ReviewRequestNode>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewRequestNode {
    #[serde(rename = "requestedReviewer")]
    pub requested_reviewer: Option<RequestedReviewer>,
}

#[derive(Debug, Deserialize)]
pub struct RequestedReviewer {
    pub login: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommitConnection {
    #[serde(default)]
    pub nodes: Vec<CommitNode>,
}

#[derive(Debug, Deserialize)]
pub struct CommitNode {
    pub commit: CommitDetail,
}

#[derive(Debug, Deserialize)]
pub struct CommitDetail {
    #[serde(rename = "statusCheckRollup")]
    pub status_check_rollup: Option<StatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
pub struct StatusCheckRollup {
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct CommentConnection {
    #[serde(default)]
    pub nodes: Vec<CommentNode>,
}

#[derive(Debug, Deserialize)]
pub struct CommentNode {
    pub id: String,
    pub author: Option<Author>,
    pub body: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewConnection {
    #[serde(default)]
    pub nodes: Vec<ReviewNode>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewNode {
    pub id: String,
    pub author: Option<Author>,
    pub body: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub state: String,
    #[serde(rename = "submittedAt")]
    pub submitted_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LatestReviewConnection {
    #[serde(default)]
    pub nodes: Vec<LatestReviewNode>,
}

#[derive(Debug, Deserialize)]
pub struct LatestReviewNode {
    pub state: String,
    #[serde(rename = "submittedAt")]
    pub submitted_at: Option<String>,
    pub author: Option<Author>,
}

pub fn build_tracked_pull_requests_search_query(
    repo_name: &str,
    authors: &[String],
    updated_after: Option<&str>,
) -> String {
    let mut terms = vec![format!("repo:{repo_name}"), "is:pr".to_string()];

    if let Some(updated_after) = updated_after {
        terms.push(format!("updated:>={updated_after}"));
    }

    terms.extend(authors.iter().map(|author| format!("author:{author}")));
    terms.push("sort:updated-desc".to_string());

    terms.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tracked_pull_requests_search_query_includes_repo_authors_and_sort() {
        let query = build_tracked_pull_requests_search_query(
            "owner/repo",
            &["alice".to_string(), "bob".to_string()],
            None,
        );

        assert!(query.contains("repo:owner/repo"));
        assert!(query.contains("is:pr"));
        assert!(!query.contains("is:open"));
        assert!(query.contains("author:alice"));
        assert!(query.contains("author:bob"));
        assert!(query.contains("sort:updated-desc"));
    }

    #[test]
    fn build_tracked_pull_requests_search_query_includes_cutoff_when_present() {
        let query = build_tracked_pull_requests_search_query(
            "owner/repo",
            &["alice".to_string()],
            Some("2026-03-25T01:55:42Z"),
        );

        assert!(query.contains("updated:>=2026-03-25T01:55:42Z"));
    }
}
