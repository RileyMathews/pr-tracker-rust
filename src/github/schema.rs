use serde::Deserialize;

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
