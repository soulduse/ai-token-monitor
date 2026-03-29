-- Add device_id column for multi-device leaderboard aggregation.
-- Existing rows default to 'default' for backward compatibility.

ALTER TABLE daily_snapshots
ADD COLUMN IF NOT EXISTS device_id text NOT NULL DEFAULT 'default';

-- Keep old (user_id, date, provider) constraint for backward compatibility
-- with older clients. Add new 4-column constraint for multi-device upsert.
ALTER TABLE daily_snapshots
ADD CONSTRAINT daily_snapshots_user_id_date_provider_device_key
UNIQUE (user_id, date, provider, device_id);
