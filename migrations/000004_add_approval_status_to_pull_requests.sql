ALTER TABLE pull_requests ADD COLUMN approval_status INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pull_requests ADD COLUMN last_review_status_update_unix INTEGER NOT NULL DEFAULT 0;
