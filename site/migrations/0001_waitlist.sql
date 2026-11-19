-- The waitlist for the macOS and Linux versions (docs/decisions/0034-site-worker-and-measuring.md).
-- One row per person and system: a person who wants both picks both, and each count stays honest.
-- source, medium and campaign say where the person came from (worker/source.ts), as visits does.
CREATE TABLE waitlist (
  email TEXT NOT NULL,
  os TEXT NOT NULL CHECK (os IN ('macos', 'linux')),
  name TEXT NOT NULL,
  joined_at TEXT NOT NULL,
  source TEXT NOT NULL,
  medium TEXT,
  campaign TEXT,
  PRIMARY KEY (email, os)
);
