-- Replace sync_device_snapshots() body with the v2 bulk-UPSERT implementation.
--
-- The v2 function (20260417020000_sync_device_snapshots_bulk.sql) was deployed
-- in parallel and verified against v1 via _test_sync_equivalence.sql — the
-- EXCEPT-based diff returned 0 rows across 7 scenarios (single insert, multi-
-- device merge, bulk 3-date upload, sole-device stale delete, multi-device
-- stale prune, device update, non-existent stale date).
--
-- This migration flips the production RPC to the bulk path by rewriting the
-- existing sync_device_snapshots() function. Clients continue calling the same
-- name — no client change required. sync_device_snapshots_v2 is kept for now
-- as an explicit alias for rollback parity and will be dropped in a follow-up
-- once the bulk path has been observed in production for 24-48h.

create or replace function sync_device_snapshots(
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
  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
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

revoke all on function sync_device_snapshots(text, text, jsonb, date[]) from public;
grant execute on function sync_device_snapshots(text, text, jsonb, date[]) to authenticated;
