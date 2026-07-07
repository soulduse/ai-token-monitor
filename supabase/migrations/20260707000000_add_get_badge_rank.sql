-- Create the get_badge_rank() RPC that the badge edge function has been calling
-- since launch but which never existed in any environment.
--
-- Impact of the missing function, per badge request (cache miss):
--   1. a wasted failed RPC round-trip (`get_badge_rank` → undefined function), then
--   2. a fallback call to get_leaderboard_entries() that aggregates the FULL
--      leaderboard just to locate one user's index, and
--   3. a correctness bug: get_leaderboard_entries() is capped at LIMIT 200, so any
--      user ranked below 200 rendered as "Unranked".
--
-- This function computes a single user's rank directly. Ordering (total desc,
-- user_id asc) and the leaderboard_hidden filter mirror get_leaderboard_entries()
-- exactly so the badge rank always matches the in-app leaderboard position.
-- Returns zero rows when the user has no data in range (the edge function treats
-- that as "Unranked").

create or replace function get_badge_rank(
  p_user_id uuid,
  p_provider text,
  p_date_from date,
  p_date_to date
) returns table (
  rank integer
)
language sql
security definer
set search_path = public
as $$
  with totals as (
    select
      s.user_id,
      sum(s.total_tokens)::bigint as total_tokens
    from daily_snapshots s
    join profiles p on p.id = s.user_id
    where s.provider = p_provider
      and s.date >= p_date_from
      and s.date <= p_date_to
      and p.leaderboard_hidden = false
    group by s.user_id
  ),
  ranked as (
    select
      t.user_id,
      row_number() over (
        order by t.total_tokens desc, t.user_id
      )::integer as rank
    from totals t
  )
  select r.rank
  from ranked r
  where r.user_id = p_user_id;
$$;

revoke all on function get_badge_rank(uuid, text, date, date) from public;
grant execute on function get_badge_rank(uuid, text, date, date) to authenticated, service_role;
