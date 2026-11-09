-- Per-user current streak for MiniProfile, computed over the user's full
-- daily_snapshots history.
--
-- Why: MiniProfile computed other users' streaks client-side from
-- get_user_activity(p_weeks => 8), which only returns the last 56 days — so
-- every other user's streak was capped at 56. The viewer's own profile reads
-- local stats and was unaffected (user report 2026-11-04).
--
-- Semantics mirror computeStreaks() in src/lib/statsHelpers.ts:
--   * a day is active when the summed total_tokens across providers/devices > 0
--   * anchor = p_today if active, else p_today - 1 if active, else streak = 0
--   * streak = length of the consecutive run of active days ending at anchor
-- p_today is the viewer's local date (the client passes it) so the result
-- matches the viewer's own "today", not the server's UTC current_date.
-- Respects leaderboard_hidden like get_user_activity (hidden → 0).

create or replace function get_user_streak(
  p_user_id uuid,
  p_today date
) returns integer
language sql
stable
security definer
set search_path = public
as $$
  with active as (
    select s.date
    from daily_snapshots s
    join profiles p on p.id = s.user_id
    where s.user_id = p_user_id
      and p.leaderboard_hidden = false
      and s.date <= p_today
    group by s.date
    having sum(s.total_tokens) > 0
  ),
  islands as (
    -- Consecutive dates share the same (date - row_number) value.
    select date, date - (row_number() over (order by date))::integer as grp
    from active
  ),
  anchor as (
    select grp
    from islands
    where date in (p_today, p_today - 1)
    order by date desc
    limit 1
  )
  select coalesce(
    (select count(*)::integer from islands i join anchor a on a.grp = i.grp),
    0
  );
$$;

revoke all on function get_user_streak(uuid, date) from public;
grant execute on function get_user_streak(uuid, date) to authenticated;
