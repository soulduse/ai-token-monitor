alter table daily_snapshots
add column if not exists device_snapshots jsonb;

do $$
begin
  if exists (
    select 1
    from information_schema.columns
    where table_schema = 'public'
      and table_name = 'daily_snapshots'
      and column_name = 'device_id'
  ) then
    create temp table _merged_daily_snapshots on commit drop as
    select
      user_id,
      date,
      provider,
      case
        when count(*) filter (where device_id is not null) > 0 then
          jsonb_object_agg(
            device_id,
            jsonb_build_object(
              'total_tokens', total_tokens,
              'cost_usd', cost_usd,
              'messages', messages,
              'sessions', sessions,
              'submitted_at', submitted_at
            )
          ) filter (where device_id is not null)
        else null
      end as device_snapshots,
      sum(total_tokens)::bigint as total_tokens,
      sum(cost_usd)::numeric(10,4) as cost_usd,
      sum(messages)::integer as messages,
      sum(sessions)::integer as sessions,
      max(submitted_at) as submitted_at
    from daily_snapshots
    group by user_id, date, provider;

    delete from daily_snapshots;

    insert into daily_snapshots (
      user_id,
      date,
      provider,
      device_snapshots,
      total_tokens,
      cost_usd,
      messages,
      sessions,
      submitted_at
    )
    select
      user_id,
      date,
      provider,
      device_snapshots,
      total_tokens,
      cost_usd,
      messages,
      sessions,
      submitted_at
    from _merged_daily_snapshots;

    drop index if exists daily_snapshots_legacy_uq;
    drop index if exists daily_snapshots_device_uq;
    drop index if exists idx_snapshots_provider_date_user_device;

    alter table daily_snapshots
    drop column if exists device_id;
  end if;
end $$;

alter table daily_snapshots
drop constraint if exists daily_snapshots_user_id_date_key;

alter table daily_snapshots
drop constraint if exists daily_snapshots_user_id_date_provider_key;

alter table daily_snapshots
add constraint daily_snapshots_user_id_date_provider_key
unique (user_id, date, provider);

drop index if exists idx_snapshots_date_provider;

create index if not exists idx_snapshots_date_provider
on daily_snapshots(date, provider);

drop policy if exists "snapshots_insert" on daily_snapshots;
drop policy if exists "snapshots_update" on daily_snapshots;
drop policy if exists "snapshots_delete" on daily_snapshots;

create policy "snapshots_insert" on daily_snapshots
for insert
with check (auth.uid() = user_id);

create policy "snapshots_update" on daily_snapshots
for update
using (auth.uid() = user_id)
with check (auth.uid() = user_id);

create policy "snapshots_delete" on daily_snapshots
for delete
using (auth.uid() = user_id);

create or replace function snapshot_totals(p_snapshots jsonb)
returns table (
  total_tokens bigint,
  cost_usd numeric(10,4),
  messages integer,
  sessions integer
)
language sql
immutable
set search_path = public
as $$
  select
    coalesce(sum((value->>'total_tokens')::bigint), 0)::bigint as total_tokens,
    coalesce(sum((value->>'cost_usd')::numeric), 0)::numeric(10,4) as cost_usd,
    coalesce(sum((value->>'messages')::integer), 0)::integer as messages,
    coalesce(sum((value->>'sessions')::integer), 0)::integer as sessions
  from jsonb_each(coalesce(p_snapshots, '{}'::jsonb));
$$;

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
  v_user_id uuid;
  v_row jsonb;
  v_date date;
  v_existing_snapshots jsonb;
  v_next_snapshots jsonb;
  v_existing_total_tokens bigint;
  v_existing_cost_usd numeric(10,4);
  v_existing_messages integer;
  v_existing_sessions integer;
  v_existing_submitted_at timestamptz;
  v_total_tokens bigint;
  v_cost_usd numeric(10,4);
  v_messages integer;
  v_sessions integer;
  v_device_payload jsonb;
begin
  v_user_id := auth.uid();

  if v_user_id is null then
    raise exception 'Not authenticated';
  end if;

  if p_provider not in ('claude', 'codex') then
    raise exception 'Invalid provider';
  end if;

  if p_device_id is null or btrim(p_device_id) = '' then
    raise exception 'Missing device_id';
  end if;

  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
    foreach v_date in array p_stale_dates loop
      select
        device_snapshots,
        total_tokens,
        cost_usd,
        messages,
        sessions,
        submitted_at
      into
        v_existing_snapshots,
        v_existing_total_tokens,
        v_existing_cost_usd,
        v_existing_messages,
        v_existing_sessions,
        v_existing_submitted_at
      from daily_snapshots
      where user_id = v_user_id
        and provider = p_provider
        and date = v_date
      for update;

      if not found or v_existing_snapshots is null then
        continue;
      end if;

      v_next_snapshots := v_existing_snapshots - p_device_id;

      select coalesce(jsonb_object_agg(key, value), '{}'::jsonb)
      into v_next_snapshots
      from jsonb_each(v_next_snapshots)
      where coalesce((value->>'submitted_at')::timestamptz, now()) >= now() - interval '30 days';

      if v_next_snapshots = '{}'::jsonb then
        delete from daily_snapshots
        where user_id = v_user_id
          and provider = p_provider
          and date = v_date;
      else
        select *
        into v_total_tokens, v_cost_usd, v_messages, v_sessions
        from snapshot_totals(v_next_snapshots);

        update daily_snapshots
        set
          device_snapshots = v_next_snapshots,
          total_tokens = v_total_tokens,
          cost_usd = v_cost_usd,
          messages = v_messages,
          sessions = v_sessions,
          submitted_at = now()
        where user_id = v_user_id
          and provider = p_provider
          and date = v_date;
      end if;
    end loop;
  end if;

  for v_row in
    select value
    from jsonb_array_elements(coalesce(p_rows, '[]'::jsonb))
  loop
    v_date := (v_row->>'date')::date;

    insert into daily_snapshots (
      user_id,
      date,
      provider,
      total_tokens,
      cost_usd,
      messages,
      sessions,
      device_snapshots,
      submitted_at
    )
    values (
      v_user_id,
      v_date,
      p_provider,
      0,
      0,
      0,
      0,
      '{}'::jsonb,
      now()
    )
    on conflict (user_id, date, provider) do nothing;

    select
      device_snapshots,
      total_tokens,
      cost_usd,
      messages,
      sessions,
      submitted_at
    into
      v_existing_snapshots,
      v_existing_total_tokens,
      v_existing_cost_usd,
      v_existing_messages,
      v_existing_sessions,
      v_existing_submitted_at
    from daily_snapshots
    where user_id = v_user_id
      and provider = p_provider
      and date = v_date
    for update;

    if v_existing_snapshots is null and (
      coalesce(v_existing_total_tokens, 0) <> 0 or
      coalesce(v_existing_cost_usd, 0) <> 0 or
      coalesce(v_existing_messages, 0) <> 0 or
      coalesce(v_existing_sessions, 0) <> 0
    ) then
      v_existing_snapshots := jsonb_build_object(
        '__legacy__',
        jsonb_build_object(
          'total_tokens', coalesce(v_existing_total_tokens, 0),
          'cost_usd', coalesce(v_existing_cost_usd, 0),
          'messages', coalesce(v_existing_messages, 0),
          'sessions', coalesce(v_existing_sessions, 0),
          'submitted_at', coalesce(v_existing_submitted_at, now())
        )
      );
    end if;

    v_device_payload := jsonb_build_object(
      'total_tokens', coalesce((v_row->>'total_tokens')::bigint, 0),
      'cost_usd', coalesce((v_row->>'cost_usd')::numeric(10,4), 0),
      'messages', coalesce((v_row->>'messages')::integer, 0),
      'sessions', coalesce((v_row->>'sessions')::integer, 0),
      'submitted_at', now()
    );

    v_next_snapshots := jsonb_set(
      coalesce(v_existing_snapshots, '{}'::jsonb),
      array[p_device_id],
      v_device_payload,
      true
    );

    -- Remove __legacy__ entry: it was a placeholder for pre-multi-device data
    -- from the same single device, now superseded by real device submissions.
    v_next_snapshots := v_next_snapshots - '__legacy__';

    select coalesce(jsonb_object_agg(key, value), '{}'::jsonb)
    into v_next_snapshots
    from jsonb_each(v_next_snapshots)
    where coalesce((value->>'submitted_at')::timestamptz, now()) >= now() - interval '30 days';

    select *
    into v_total_tokens, v_cost_usd, v_messages, v_sessions
    from snapshot_totals(v_next_snapshots);

    update daily_snapshots
    set
      device_snapshots = v_next_snapshots,
      total_tokens = v_total_tokens,
      cost_usd = v_cost_usd,
      messages = v_messages,
      sessions = v_sessions,
      submitted_at = now()
    where user_id = v_user_id
      and provider = p_provider
      and date = v_date;
  end loop;
end;
$$;

create or replace function get_leaderboard_entries(
  p_provider text,
  p_date_from date,
  p_date_to date
) returns table (
  user_id uuid,
  nickname text,
  avatar_url text,
  total_tokens bigint,
  cost_usd numeric(10,4),
  messages integer,
  sessions integer
)
language sql
security definer
set search_path = public
as $$
  select
    s.user_id,
    p.nickname,
    p.avatar_url,
    sum(s.total_tokens)::bigint as total_tokens,
    sum(s.cost_usd)::numeric(10,4) as cost_usd,
    sum(s.messages)::integer as messages,
    sum(s.sessions)::integer as sessions
  from daily_snapshots s
  join profiles p on p.id = s.user_id
  where s.provider = p_provider
    and s.date >= p_date_from
    and s.date <= p_date_to
  group by s.user_id, p.nickname, p.avatar_url
  order by sum(s.total_tokens) desc, s.user_id;
$$;

revoke all on function snapshot_totals(jsonb) from public;

revoke all on function sync_device_snapshots(text, text, jsonb, date[]) from public;
grant execute on function sync_device_snapshots(text, text, jsonb, date[]) to authenticated;

revoke all on function get_leaderboard_entries(text, date, date) from public;
grant execute on function get_leaderboard_entries(text, date, date) to authenticated;
