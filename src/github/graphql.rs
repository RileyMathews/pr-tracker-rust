use serde::Deserialize;

pub const OPEN_PULL_REQUESTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: [OPEN], first: 100, after: $cursor) {
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
    pub author: Option<Author>,
    #[serde(rename = "reviewRequests")]
    pub review_requests: ReviewRequestConnection,
    #[serde(rename = "headRefOid")]
    pub head_ref_oid: String,
    pub commits: CommitConnection,
    pub comments: CommentConnection,
    pub reviews: ReviewConnection,
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
