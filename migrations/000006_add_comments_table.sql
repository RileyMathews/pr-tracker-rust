CREATE TABLE IF NOT EXISTS pr_comments (
  id TEXT PRIMARY KEY,
  repository TEXT NOT NULL,
  pr_number INTEGER NOT NULL,
  author TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at_unix INTEGER NOT NULL,
  updated_at_unix INTEGER NOT NULL,
  is_review_comment BOOLEAN NOT NULL DEFAULT 0,
  review_state TEXT,
  FOREIGN KEY (repository, pr_number) REFERENCES pull_requests(repository, number) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pr_comments_repository_pr_number ON pr_comments(repository, pr_number);
CREATE INDEX IF NOT EXISTS idx_pr_comments_author ON pr_comments(author);
CREATE INDEX IF NOT EXISTS idx_pr_comments_created_at_unix ON pr_comments(created_at_unix);
