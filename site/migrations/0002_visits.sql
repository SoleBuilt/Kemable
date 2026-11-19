-- Where the website's visitors come from (docs/decisions/0034-site-worker-and-measuring.md): one row per
-- page view or Download click. No IP address, no browser details and no identifier: a row cannot be
-- tied to a person, so counting needs no consent.
CREATE TABLE visits (
  at TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('view', 'download')),
  path TEXT NOT NULL,
  -- 'direct' when neither a UTM tag, an ad click id nor a referring site says.
  source TEXT NOT NULL,
  -- 'ai', 'search', 'social', 'referral', or the link's own utm_medium ('cpc', 'email'...).
  medium TEXT,
  campaign TEXT,
  -- The referring site's host only, never its path.
  referrer TEXT,
  -- Two letters, from Cloudflare.
  country TEXT
);

CREATE INDEX visits_by_time ON visits (at);
