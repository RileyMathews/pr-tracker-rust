use serde::Deserialize;

pub const OPEN_PULL_REQUESTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: [OPEN], first: 100, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo {
        hasNextPage
        endCursor
      }
      nodes {
        number
        title
        isDraft
        createdAt
        updatedAt
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
            updatedAt
          }
        }
        reviews(last: 100) {
          nodes {
            updatedAt
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
pub struct QueryResponse {
    pub repository: Option<Repository>,
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    #[serde(rename = "pullRequests")]
    pub pull_requests: PullRequestConnection,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestConnection {
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
    #[serde(default)]
    pub state: Option<String>,
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
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
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

pub const DISCOVERY_PULL_REQUESTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: [OPEN], first: 100, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo {
        hasNextPage
        endCursor
      }
      nodes {
        number
        updatedAt
        author {
          login
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub struct DiscoveryQueryResponse {
    pub repository: Option<DiscoveryRepository>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryRepository {
    #[serde(rename = "pullRequests")]
    pub pull_requests: DiscoveryPullRequestConnection,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryPullRequestConnection {
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    #[serde(default)]
    pub nodes: Vec<DiscoveryPullRequestNode>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryPullRequestNode {
    pub number: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub author: Option<Author>,
}

pub fn build_targeted_refresh_query(pr_numbers: &[i64]) -> String {
    let mut fields = String::new();
    for &number in pr_numbers.iter().filter(|&&n| n > 0) {
        use std::fmt::Write;
        write!(
            fields,
            r#"
    pr_{number}: pullRequest(number: {number}) {{
      number
      title
      isDraft
      createdAt
      updatedAt
      headRefOid
      state
      author {{
        login
      }}
      reviewRequests(first: 100) {{
        nodes {{
          requestedReviewer {{
            ... on User {{
              login
            }}
          }}
        }}
      }}
      commits(last: 1) {{
        nodes {{
          commit {{
            statusCheckRollup {{
              state
            }}
          }}
        }}
      }}
      comments(last: 100) {{
        nodes {{
          updatedAt
        }}
      }}
      reviews(last: 100) {{
        nodes {{
          updatedAt
        }}
      }}
      latestReviews(first: 100) {{
        nodes {{
          state
          submittedAt
          author {{
            login
          }}
        }}
      }}
    }}"#,
        )
        .unwrap();
    }

    format!(
        r#"query($owner: String!, $name: String!) {{
  repository(owner: $owner, name: $name) {{{fields}
  }}
}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_targeted_refresh_query_single_pr() {
        let query = build_targeted_refresh_query(&[42]);
        assert!(query.contains("pr_42: pullRequest(number: 42)"));
        assert!(query.contains("state"));
        assert!(query.contains("statusCheckRollup"));
        assert!(query.contains("$owner: String!"));
        assert!(query.contains("$name: String!"));
    }

    #[test]
    fn build_targeted_refresh_query_multiple_prs() {
        let query = build_targeted_refresh_query(&[1, 2, 3]);
        assert!(query.contains("pr_1: pullRequest(number: 1)"));
        assert!(query.contains("pr_2: pullRequest(number: 2)"));
        assert!(query.contains("pr_3: pullRequest(number: 3)"));
    }

    #[test]
    fn build_targeted_refresh_query_empty() {
        let query = build_targeted_refresh_query(&[]);
        // Should still be a valid query structure, just with no PR fields
        assert!(query.contains("repository(owner: $owner, name: $name)"));
        assert!(!query.contains("pullRequest"));
    }

    #[test]
    fn build_targeted_refresh_query_filters_non_positive() {
        let query = build_targeted_refresh_query(&[-1, 0, 5]);
        assert!(!query.contains("pr_-1"));
        assert!(!query.contains("pr_0"));
        assert!(query.contains("pr_5: pullRequest(number: 5)"));
    }
}
