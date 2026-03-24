-- Add provider column to support multiple AI coding tools (Claude Code, Codex, etc.)
-- Existing rows default to 'claude' for backward compatibility.

alter table daily_snapshots
add column if not exists provider text not null default 'claude';

-- Drop old unique constraint and create new one including provider
alter table daily_snapshots
drop constraint if exists daily_snapshots_user_id_date_key;

alter table daily_snapshots
add constraint daily_snapshots_user_id_date_provider_key
unique (user_id, date, provider);

-- Validate provider values
alter table daily_snapshots
drop constraint if exists daily_snapshots_provider_check;

alter table daily_snapshots
add constraint daily_snapshots_provider_check
check (provider in ('claude', 'codex'));

-- Index for provider-filtered queries
create index if not exists idx_snapshots_date_provider
on daily_snapshots(date, provider);
