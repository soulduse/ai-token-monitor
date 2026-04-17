-- Rewrite sync_device_snapshots() from a row-by-row PL/pgSQL loop into a
-- set-based bulk UPSERT + bulk stale cleanup.
--
-- Original structure executed INSERT + SELECT FOR UPDATE + UPDATE per row,
-- plus a separate per-date loop for stale cleanup. A 60-day backfill meant
-- ~180 DB ops; automatic today-only uploads still hit 3 ops. Per 67 active
-- users × 2 providers this dominated Disk IO burst on Nano.
--
-- New structure uses two set-based statements:
--
--   1) Bulk stale cleanup: a single UPDATE against all p_stale_dates at once,
--      with a CASE that either strips the device entry or deletes the row
--      when no entries remain after the 30-day cutoff.
--
--   2) Bulk upsert: one INSERT ... SELECT ... ON CONFLICT DO UPDATE that
--      takes jsonb_array_elements(p_rows), merges the new device entry into
--      the existing device_snapshots JSONB (with 30-day cutoff applied),
--      and recomputes totals via snapshot_totals() in the same statement.
--
-- Correctness invariants preserved from the original:
--   - auth.uid() must match row user_id
--   - provider whitelist ('claude','codex','opencode')
--   - 30-day cutoff on device_snapshots entries (based on submitted_at)
--   - __legacy__ placeholder is dropped after merge
--   - snapshot_totals() remains the single source of truth for aggregate fields
--   - Row is deleted when all device entries are evicted (all older than 30d)
--
-- Expected reduction: 60-day backfill 180 → 2 ops (99%), auto-upload 3 → 1 ops.

create or replace function sync_device_snapshots_v2(
  p_provider text,
  p_device_id text,
  p_rows jsonb default '[]'::jsonb,
  p_stale_dates date[] default '{}'::date[]
) returns void
language plpgsql
security definer
set search_path = public
as $$
declare
  v_user_id uuid := auth.uid();
begin
  if v_user_id is null then
    raise exception 'Not authenticated';
  end if;

  if p_provider not in ('claude', 'codex', 'opencode') then
    raise exception 'Invalid provider';
  end if;

  if p_device_id is null or btrim(p_device_id) = '' then
    raise exception 'Missing device_id';
  end if;

  -- Step 1: bulk stale cleanup.
  -- For each stale date, remove this device's entry and re-apply the 30-day
  -- cutoff. Rows with nothing left after pruning are deleted; others get
  -- totals recomputed. Two statements because CTEs inside UPDATE cannot
  -- guarantee a sibling DELETE runs when it isn't referenced.
  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
    -- 1a: delete rows where pruning would leave no device entries.
    delete from daily_snapshots d
    where d.user_id = v_user_id
      and d.provider = p_provider
      and d.date = any(p_stale_dates)
      and coalesce(
        (
          select jsonb_object_agg(key, value)
          from jsonb_each(coalesce(d.device_snapshots, '{}'::jsonb) - p_device_id) e
          where coalesce((e.value->>'submitted_at')::timestamptz, now())
                >= now() - interval '30 days'
        ),
        '{}'::jsonb
      ) = '{}'::jsonb;

    -- 1b: update surviving rows with pruned device_snapshots and fresh totals.
    with pruned as (
      select
        s.user_id,
        s.date,
        s.provider,
        (
          select jsonb_object_agg(key, value)
          from jsonb_each(coalesce(s.device_snapshots, '{}'::jsonb) - p_device_id) e
          where coalesce((e.value->>'submitted_at')::timestamptz, now())
                >= now() - interval '30 days'
        ) as next_snapshots
      from daily_snapshots s
      where s.user_id = v_user_id
        and s.provider = p_provider
        and s.date = any(p_stale_dates)
      for update
    )
    update daily_snapshots d
    set
      device_snapshots = p.next_snapshots,
      total_tokens = t.total_tokens,
      cost_usd = t.cost_usd,
      messages = t.messages,
      sessions = t.sessions,
      submitted_at = now()
    from pruned p, lateral snapshot_totals(p.next_snapshots) t
    where d.user_id = p.user_id
      and d.provider = p.provider
      and d.date = p.date
      and p.next_snapshots is not null;
  end if;

  -- Step 2: bulk upsert.
  -- For each incoming row, merge the new device entry into device_snapshots,
  -- drop __legacy__, apply the 30-day cutoff, and recompute totals — all in
  -- a single INSERT ... ON CONFLICT DO UPDATE.
  if p_rows is not null and jsonb_array_length(p_rows) > 0 then
    with incoming as (
      select
        (row_data->>'date')::date as date,
        jsonb_build_object(
          'total_tokens', coalesce((row_data->>'total_tokens')::bigint, 0),
          'cost_usd',     coalesce((row_data->>'cost_usd')::numeric(10,4), 0),
          'messages',     coalesce((row_data->>'messages')::integer, 0),
          'sessions',     coalesce((row_data->>'sessions')::integer, 0),
          'submitted_at', now()
        ) as device_payload
      from jsonb_array_elements(coalesce(p_rows, '[]'::jsonb)) row_data
    ),
    merged as (
      select
        i.date,
        -- Merge order: start from existing snapshots (or empty), drop __legacy__,
        -- overlay the incoming device entry via jsonb_set, then apply the
        -- 30-day cutoff on submitted_at.
        coalesce(
          (
            select jsonb_object_agg(key, value)
            from jsonb_each(
              jsonb_set(
                coalesce(existing.device_snapshots, '{}'::jsonb) - '__legacy__',
                array[p_device_id],
                i.device_payload,
                true
              )
            ) e
            where coalesce((e.value->>'submitted_at')::timestamptz, now())
                  >= now() - interval '30 days'
          ),
          jsonb_build_object(p_device_id, i.device_payload)
        ) as next_snapshots
      from incoming i
      left join daily_snapshots existing
        on existing.user_id = v_user_id
       and existing.provider = p_provider
       and existing.date = i.date
    )
    insert into daily_snapshots (
      user_id, date, provider,
      total_tokens, cost_usd, messages, sessions,
      device_snapshots, submitted_at
    )
    select
      v_user_id,
      m.date,
      p_provider,
      t.total_tokens,
      t.cost_usd,
      t.messages,
      t.sessions,
      m.next_snapshots,
      now()
    from merged m, lateral snapshot_totals(m.next_snapshots) t
    on conflict (user_id, date, provider) do update set
      device_snapshots = excluded.device_snapshots,
      total_tokens     = excluded.total_tokens,
      cost_usd         = excluded.cost_usd,
      messages         = excluded.messages,
      sessions         = excluded.sessions,
      submitted_at     = excluded.submitted_at;
  end if;
end;
$$;

revoke all on function sync_device_snapshots_v2(text, text, jsonb, date[]) from public;
grant execute on function sync_device_snapshots_v2(text, text, jsonb, date[]) to authenticated;
