-- What told the source of a visit or a waitlist entry (worker/source.ts, Source.signal), so a count
-- can be split by it: 'utm' (the link's utm_source), 'click_id' (gclid, msclkid, fbclid), 'referrer'
-- (the referring site), 'none' (direct). Empty for rows written before 2026-11-18, of which there were none.
ALTER TABLE visits ADD COLUMN signal TEXT CHECK (signal IN ('utm', 'click_id', 'referrer', 'none'));
ALTER TABLE waitlist ADD COLUMN signal TEXT CHECK (signal IN ('utm', 'click_id', 'referrer', 'none'));
