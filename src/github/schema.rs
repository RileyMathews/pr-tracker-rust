use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Reviewer {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub login: String,
    pub id: i64,
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub draft: bool,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub user: PullRequestUser,
    pub requested_reviewers: Vec<Reviewer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueComment {
    pub id: i64,
    pub body: String,
    pub html_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub user: PullRequestUser,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewComment {
    pub id: i64,
    pub body: String,
    pub html_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub path: Option<String>,
    pub user: PullRequestUser,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestDetails {
    #[serde(flatten)]
    pub pull_request: PullRequest,
    #[serde(rename = "comments")]
    pub issue_comment_count: i64,
    #[serde(rename = "review_comments")]
    pub review_comment_count: i64,
    #[serde(skip)]
    pub issue_comments: Vec<IssueComment>,
    #[serde(skip)]
    pub review_comments: Vec<ReviewComment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitStatusContext {
    pub context: String,
    pub state: String,
    pub description: Option<String>,
    pub target_url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    pub id: i64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: Option<String>,
    pub details_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub app: CheckRunApp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRunApp {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestCiStatuses {
    pub pull_request_number: i64,
    pub head_sha: String,
    pub combined_state: String,
    pub statuses: Vec<CommitStatusContext>,
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestHead {
    pub number: i64,
    pub head: PullRequestHeadSha,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestHeadSha {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CombinedStatus {
    pub state: String,
    pub statuses: Vec<CommitStatusContext>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRunsPage {
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamOrg {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserTeam {
    pub slug: String,
    pub name: String,
    pub organization: TeamOrg,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamMember {
    pub login: String,
}
