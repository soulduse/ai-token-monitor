-- Fix: get_user_activity RLS bypass for own profile.
-- Previous version (20260411010000) blocked own activity if leaderboard_hidden = true.
create or replace function get_user_activity(
  p_user_id uuid,
  p_weeks integer default 8
) returns table (
  date date,
  total_tokens bigint,
  cost_usd numeric(10,4),
  messages integer,
  sessions integer
)
language sql
security definer
set search_path = public
as $func$
  select
    s.date,
    sum(s.total_tokens)::bigint as total_tokens,
    sum(s.cost_usd)::numeric(10,4) as cost_usd,
    sum(s.messages)::integer as messages,
    sum(s.sessions)::integer as sessions
  from daily_snapshots s
  join profiles p on p.id = s.user_id
  where s.user_id = p_user_id
    and (
      auth.uid() = p_user_id
      or
      p.leaderboard_hidden = false
    )
    and s.date >= current_date - (p_weeks * 7 - 1)
  group by s.date
  order by s.date asc;
$func$;

revoke all on function get_user_activity(uuid, integer) from public;
grant execute on function get_user_activity(uuid, integer) to authenticated;
