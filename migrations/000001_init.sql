CREATE TABLE IF NOT EXISTS pull_requests (
  number INTEGER NOT NULL,
  title TEXT NOT NULL,
  repository TEXT NOT NULL,
  author TEXT NOT NULL,
  draft BOOLEAN NOT NULL,
  created_at_unix INTEGER NOT NULL,
  updated_at_unix INTEGER NOT NULL,
  ci_status INTEGER NOT NULL,
  last_comment_unix INTEGER NOT NULL,
  last_commit_unix INTEGER NOT NULL,
  last_ci_status_update_unix INTEGER NOT NULL,
  last_acknowledged_unix INTEGER,
  requested_reviewers TEXT NOT NULL DEFAULT '[]',
  PRIMARY KEY (repository, number)
);

CREATE TABLE IF NOT EXISTS tracked_authors (
  author TEXT NOT NULL PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS tracked_repositories (
  repository TEXT NOT NULL PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL,
  access_token TEXT NOT NULL UNIQUE
);
