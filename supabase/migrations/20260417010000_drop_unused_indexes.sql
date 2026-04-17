-- Drop unused indexes on daily_snapshots to reduce WAL amplification per UPDATE.
--
-- Rationale: pg_stat_user_indexes shows idx_scan=0 for idx_snapshots_date
-- despite daily_snapshots receiving 203k+ total index scans. Each UPDATE
-- currently maintains 5 indexes — every write amplifies WAL and burns Disk
-- IO burst budget on the Nano instance. idx_snapshots_date is fully shadowed
-- by idx_snapshots_date_provider(date,provider) via btree prefix matching.
--
-- Step 1 (this migration): drop unused secondary index and swap the PK from
-- the synthetic id column to the natural composite key. The id column is
-- retained for now as a rollback safety net — it will be dropped in a
-- follow-up migration once the schema change has been observed for 24-48h.
--
-- Code/constraint audit confirms id is unreferenced:
--   - 0 matches in src/ and src-tauri/
--   - 0 foreign keys referencing daily_snapshots.id
--   - 0 RLS policies referencing id (all use user_id)

-- Build a dedicated unique index for the natural composite key. Doing this
-- BEFORE dropping the existing unique constraint avoids a gap where writes
-- could race past uniqueness enforcement.
create unique index if not exists daily_snapshots_pk_idx
  on daily_snapshots (user_id, date, provider);

-- Drop the synthetic id-based PK
alter table daily_snapshots drop constraint if exists daily_snapshots_pkey;

-- Drop the old unique constraint (this also drops its auto-generated index,
-- which is why we created daily_snapshots_pk_idx up front).
alter table daily_snapshots
  drop constraint if exists daily_snapshots_user_id_date_provider_key;

-- Drop unused secondary index
drop index if exists idx_snapshots_date;

-- Promote the new index to primary key in place.
alter table daily_snapshots
  add constraint daily_snapshots_pkey
  primary key using index daily_snapshots_pk_idx;
