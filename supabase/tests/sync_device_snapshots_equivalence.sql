-- Equivalence test for sync_device_snapshots vs sync_device_snapshots_v2.
--
-- Creates two isolated test tables (daily_snapshots_v1_test, _v2_test) that
-- mirror the schema of daily_snapshots, wraps each sync implementation to
-- target the test tables, drives the same sequence of calls against both,
-- then diffs the resulting rows. Does NOT touch production daily_snapshots.
--
-- Run inside a single transaction so everything rolls back at the end,
-- leaving the production DB untouched.

begin;

-- 1. Mirror schema (DROP COLUMNs and constraints simplified for test isolation).
create table daily_snapshots_v1_test (
  user_id uuid not null,
  date date not null,
  provider text not null,
  total_tokens bigint not null default 0,
  cost_usd numeric(10,4) not null default 0,
  messages integer not null default 0,
  sessions integer not null default 0,
  device_snapshots jsonb,
  submitted_at timestamptz,
  primary key (user_id, date, provider)
);

create table daily_snapshots_v2_test (like daily_snapshots_v1_test including all);

-- 2. Wrap v1 / v2 logic against the test tables.
--    We inline the same logic but point at the *_test tables. This keeps the
--    test self-contained and avoids mutating production.

-- Helper to seed a user_id without auth.uid()
create or replace function _test_run_v1(
  p_target regclass, p_user_id uuid, p_provider text, p_device_id text,
  p_rows jsonb, p_stale_dates date[]
) returns void language plpgsql as $$
declare
  v_row jsonb;
  v_date date;
  v_existing jsonb;
  v_next jsonb;
  v_payload jsonb;
  v_t bigint; v_c numeric(10,4); v_m integer; v_s integer;
begin
  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
    foreach v_date in array p_stale_dates loop
      execute format($f$
        select device_snapshots from %s
         where user_id=$1 and provider=$2 and date=$3 for update
      $f$, p_target) into v_existing using p_user_id, p_provider, v_date;

      if v_existing is null then continue; end if;

      v_next := v_existing - p_device_id;
      select coalesce(jsonb_object_agg(key, value), '{}'::jsonb) into v_next
      from jsonb_each(v_next)
      where coalesce((value->>'submitted_at')::timestamptz, now()) >= now() - interval '30 days';

      if v_next = '{}'::jsonb then
        execute format('delete from %s where user_id=$1 and provider=$2 and date=$3', p_target)
          using p_user_id, p_provider, v_date;
      else
        select * into v_t, v_c, v_m, v_s from snapshot_totals(v_next);
        execute format($f$
          update %s set device_snapshots=$1, total_tokens=$2, cost_usd=$3,
                        messages=$4, sessions=$5, submitted_at=now()
          where user_id=$6 and provider=$7 and date=$8
        $f$, p_target) using v_next, v_t, v_c, v_m, v_s, p_user_id, p_provider, v_date;
      end if;
    end loop;
  end if;

  for v_row in select value from jsonb_array_elements(coalesce(p_rows, '[]'::jsonb)) loop
    v_date := (v_row->>'date')::date;

    execute format($f$
      insert into %s (user_id, date, provider, device_snapshots, submitted_at)
      values ($1,$2,$3,'{}'::jsonb, now())
      on conflict (user_id, date, provider) do nothing
    $f$, p_target) using p_user_id, v_date, p_provider;

    execute format($f$
      select device_snapshots from %s where user_id=$1 and provider=$2 and date=$3 for update
    $f$, p_target) into v_existing using p_user_id, p_provider, v_date;

    v_payload := jsonb_build_object(
      'total_tokens', coalesce((v_row->>'total_tokens')::bigint, 0),
      'cost_usd',     coalesce((v_row->>'cost_usd')::numeric(10,4), 0),
      'messages',     coalesce((v_row->>'messages')::integer, 0),
      'sessions',     coalesce((v_row->>'sessions')::integer, 0),
      'submitted_at', now()
    );

    v_next := jsonb_set(coalesce(v_existing, '{}'::jsonb), array[p_device_id], v_payload, true);
    v_next := v_next - '__legacy__';
    select coalesce(jsonb_object_agg(key, value), '{}'::jsonb) into v_next
    from jsonb_each(v_next)
    where coalesce((value->>'submitted_at')::timestamptz, now()) >= now() - interval '30 days';

    select * into v_t, v_c, v_m, v_s from snapshot_totals(v_next);

    execute format($f$
      update %s set device_snapshots=$1, total_tokens=$2, cost_usd=$3,
                    messages=$4, sessions=$5, submitted_at=now()
      where user_id=$6 and provider=$7 and date=$8
    $f$, p_target) using v_next, v_t, v_c, v_m, v_s, p_user_id, p_provider, v_date;
  end loop;
end; $$;

-- v2 wrapper: same as production v2 but parameterised on target table
create or replace function _test_run_v2(
  p_target regclass, p_user_id uuid, p_provider text, p_device_id text,
  p_rows jsonb, p_stale_dates date[]
) returns void language plpgsql as $$
begin
  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
    execute format($f$
      delete from %s d
      where d.user_id=$1 and d.provider=$2 and d.date = any($3)
        and coalesce((
          select jsonb_object_agg(key, value)
          from jsonb_each(coalesce(d.device_snapshots, '{}'::jsonb) - $4) e
          where coalesce((e.value->>'submitted_at')::timestamptz, now())
                >= now() - interval '30 days'
        ), '{}'::jsonb) = '{}'::jsonb
    $f$, p_target) using p_user_id, p_provider, p_stale_dates, p_device_id;

    execute format($f$
      with pruned as (
        select s.user_id, s.date, s.provider,
          (select jsonb_object_agg(key, value)
             from jsonb_each(coalesce(s.device_snapshots, '{}'::jsonb) - $4) e
             where coalesce((e.value->>'submitted_at')::timestamptz, now())
                   >= now() - interval '30 days') as next_snapshots
        from %s s
        where s.user_id=$1 and s.provider=$2 and s.date = any($3)
        for update
      )
      update %s d
      set device_snapshots = p.next_snapshots,
          total_tokens = t.total_tokens, cost_usd = t.cost_usd,
          messages = t.messages, sessions = t.sessions, submitted_at = now()
      from pruned p, lateral snapshot_totals(p.next_snapshots) t
      where d.user_id = p.user_id and d.provider = p.provider and d.date = p.date
        and p.next_snapshots is not null
    $f$, p_target, p_target) using p_user_id, p_provider, p_stale_dates, p_device_id;
  end if;

  if p_rows is not null and jsonb_array_length(p_rows) > 0 then
    execute format($f$
      with incoming as (
        select (row_data->>'date')::date as date,
          jsonb_build_object(
            'total_tokens', coalesce((row_data->>'total_tokens')::bigint, 0),
            'cost_usd',     coalesce((row_data->>'cost_usd')::numeric(10,4), 0),
            'messages',     coalesce((row_data->>'messages')::integer, 0),
            'sessions',     coalesce((row_data->>'sessions')::integer, 0),
            'submitted_at', now()
          ) as device_payload
        from jsonb_array_elements($4) row_data
      ),
      merged as (
        select i.date,
          coalesce((
            select jsonb_object_agg(key, value)
            from jsonb_each(
              jsonb_set(
                coalesce(existing.device_snapshots, '{}'::jsonb) - '__legacy__',
                array[$3], i.device_payload, true
              )
            ) e
            where coalesce((e.value->>'submitted_at')::timestamptz, now())
                  >= now() - interval '30 days'
          ), jsonb_build_object($3, i.device_payload)) as next_snapshots
        from incoming i
        left join %s existing
          on existing.user_id=$1 and existing.provider=$2 and existing.date=i.date
      )
      insert into %s (user_id, date, provider, total_tokens, cost_usd,
                      messages, sessions, device_snapshots, submitted_at)
      select $1, m.date, $2, t.total_tokens, t.cost_usd, t.messages, t.sessions,
             m.next_snapshots, now()
      from merged m, lateral snapshot_totals(m.next_snapshots) t
      on conflict (user_id, date, provider) do update set
        device_snapshots = excluded.device_snapshots,
        total_tokens = excluded.total_tokens, cost_usd = excluded.cost_usd,
        messages = excluded.messages, sessions = excluded.sessions,
        submitted_at = excluded.submitted_at
    $f$, p_target, p_target) using p_user_id, p_provider, p_device_id, p_rows;
  end if;
end; $$;

-- 3. Test scenarios.
-- Use a single user_id across scenarios; two device ids to cover multi-device.
do $$
declare
  u uuid := '00000000-0000-0000-0000-000000000001'::uuid;
  d1 text := 'device-alpha';
  d2 text := 'device-beta';
begin
  -- Scenario 1: insert today from device 1
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d1,
    '[{"date":"2026-04-17","total_tokens":100,"cost_usd":0.5,"messages":5,"sessions":1}]'::jsonb, '{}'::date[]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d1,
    '[{"date":"2026-04-17","total_tokens":100,"cost_usd":0.5,"messages":5,"sessions":1}]'::jsonb, '{}'::date[]);

  -- Scenario 2: add device 2 to same date (multi-device merge)
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d2,
    '[{"date":"2026-04-17","total_tokens":200,"cost_usd":1.0,"messages":10,"sessions":2}]'::jsonb, '{}'::date[]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d2,
    '[{"date":"2026-04-17","total_tokens":200,"cost_usd":1.0,"messages":10,"sessions":2}]'::jsonb, '{}'::date[]);

  -- Scenario 3: device 1 uploads 3 dates at once (bulk)
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d1, $j$[
    {"date":"2026-04-16","total_tokens":50,"cost_usd":0.2,"messages":3,"sessions":1},
    {"date":"2026-04-15","total_tokens":80,"cost_usd":0.3,"messages":4,"sessions":1},
    {"date":"2026-04-14","total_tokens":120,"cost_usd":0.6,"messages":6,"sessions":2}
  ]$j$::jsonb, '{}'::date[]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d1, $j$[
    {"date":"2026-04-16","total_tokens":50,"cost_usd":0.2,"messages":3,"sessions":1},
    {"date":"2026-04-15","total_tokens":80,"cost_usd":0.3,"messages":4,"sessions":1},
    {"date":"2026-04-14","total_tokens":120,"cost_usd":0.6,"messages":6,"sessions":2}
  ]$j$::jsonb, '{}'::date[]);

  -- Scenario 4: stale cleanup for device 1 on 2026-04-14 (sole device → row delete)
  -- First ensure device 2 never touched 2026-04-14, then remove device 1 via stale.
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d1,
    '[]'::jsonb, array['2026-04-14'::date]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d1,
    '[]'::jsonb, array['2026-04-14'::date]);

  -- Scenario 5: stale cleanup for device 1 on 2026-04-17 (device 2 survives)
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d1,
    '[]'::jsonb, array['2026-04-17'::date]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d1,
    '[]'::jsonb, array['2026-04-17'::date]);

  -- Scenario 6: update same (device, date) with new values
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d2,
    '[{"date":"2026-04-17","total_tokens":300,"cost_usd":1.5,"messages":15,"sessions":3}]'::jsonb, '{}'::date[]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d2,
    '[{"date":"2026-04-17","total_tokens":300,"cost_usd":1.5,"messages":15,"sessions":3}]'::jsonb, '{}'::date[]);

  -- Scenario 7: stale cleanup referencing a date that doesn't exist (no-op)
  perform _test_run_v1('daily_snapshots_v1_test', u, 'claude', d1,
    '[]'::jsonb, array['2020-01-01'::date]);
  perform _test_run_v2('daily_snapshots_v2_test', u, 'claude', d1,
    '[]'::jsonb, array['2020-01-01'::date]);
end $$;

-- 4. Diff. Any rows returned here are bugs.
-- Compare by tuple ignoring submitted_at (timing differs between calls).
select 'only_in_v1' as side, user_id, date, provider,
       total_tokens, cost_usd, messages, sessions,
       device_snapshots - 'submitted_at' as device_snapshots_excl_time
from (
  select user_id, date, provider, total_tokens, cost_usd, messages, sessions,
         device_snapshots
  from daily_snapshots_v1_test
  except
  select user_id, date, provider, total_tokens, cost_usd, messages, sessions,
         device_snapshots
  from daily_snapshots_v2_test
) diff
union all
select 'only_in_v2', user_id, date, provider,
       total_tokens, cost_usd, messages, sessions,
       device_snapshots - 'submitted_at'
from (
  select user_id, date, provider, total_tokens, cost_usd, messages, sessions,
         device_snapshots
  from daily_snapshots_v2_test
  except
  select user_id, date, provider, total_tokens, cost_usd, messages, sessions,
         device_snapshots
  from daily_snapshots_v1_test
) diff;

-- Roll back so nothing persists.
rollback;
