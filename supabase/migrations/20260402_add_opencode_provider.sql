-- Add 'opencode' to the provider CHECK constraint on daily_snapshots
alter table daily_snapshots
drop constraint if exists daily_snapshots_provider_check;

alter table daily_snapshots
add constraint daily_snapshots_provider_check
check (provider in ('claude', 'codex', 'opencode'));

-- Update sync_device_snapshots to accept 'opencode' as a valid provider
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

  if p_provider not in ('claude', 'codex', 'opencode') then
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
