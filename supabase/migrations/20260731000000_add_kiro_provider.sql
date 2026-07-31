-- Extend the provider whitelist on daily_snapshots to accept 'kiro' (Amazon Q
-- Developer / CodeWhisperer, via kiro-cli).
--
-- Kiro is the first provider that does not meter in tokens. It bills *credits*,
-- and records no token counts anywhere locally, so its rows carry credits in the
-- `total_tokens` column instead. Because that column is `bigint` and a day of
-- Kiro usage is a small fractional number (e.g. 20.562976 credits), the client
-- stores **millicredits** (credits x 1000) to keep three decimals of precision
-- inside the integer column. See src/lib/kiro.ts.
--
-- This is safe without a schema change because the leaderboard is scoped per
-- provider: the ranking RPC filters on p_provider and the UI is split into
-- provider tabs, so a Kiro row is never sorted against a token-based one.
--
-- Two places reject an unknown provider, and both must be widened:
--   1. the daily_snapshots_provider_check CHECK constraint
--   2. the sync_device_snapshots() RPC's provider validation
-- Without this migration, Kiro uploads fail at the RPC boundary with
-- 'Invalid provider' -- and useSnapshotUploader swallows the error
-- (`return !error`), so nothing would surface in the UI.
--
-- The function body below is copied verbatim from
-- 20260729000000_add_grok_provider.sql with only the provider lists widened,
-- which avoids accidental behavior drift.

alter table daily_snapshots
drop constraint if exists daily_snapshots_provider_check;

alter table daily_snapshots
add constraint daily_snapshots_provider_check
check (provider in ('claude', 'codex', 'opencode', 'kimi', 'glm', 'gjc', 'grok', 'kiro'));

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

  if p_provider not in ('claude', 'codex', 'opencode', 'kimi', 'glm', 'gjc', 'grok', 'kiro') then
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
