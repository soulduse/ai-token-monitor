-- Add a virtual 'combined' leaderboard view that ranks users by their summed
-- Claude + Codex usage (user request: "클로드+코덱스 합친 토큰 사용량 랭킹").
--
-- 'combined' is NOT a stored provider: daily_snapshots keeps one row per real
-- provider, and the CHECK constraint / sync_device_snapshots() whitelist stay
-- untouched, so nothing can ever be uploaded under 'combined'. Only the three
-- read RPCs learn to expand it:
--
--   get_leaderboard_entries   (list view)
--   get_leaderboard_date_grid (grid view)
--   get_badge_rank            (badge rank; must match the in-app position)
--
-- All three already sum per user (or per date+user), so widening the provider
-- filter is enough to aggregate across both providers. The set is fixed to
-- claude+codex: both count tokens including cache, while e.g. kiro stores
-- credits in total_tokens and must never be mixed in.
--
-- Bodies are copied verbatim from their latest definitions with only the
-- provider filter replaced; signatures are unchanged, so existing grants stay.
--   get_leaderboard_entries   <- 20260411000000_remove_leaderboard_outlier_filter.sql
--   get_leaderboard_date_grid <- 20260410_leaderboard_date_grid.sql
--   get_badge_rank            <- 20260707000000_add_get_badge_rank.sql

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
  where s.provider = any(
          case when p_provider = 'combined'
               then array['claude', 'codex']
               else array[p_provider] end
        )
    and s.date >= p_date_from
    and s.date <= p_date_to
    and p.leaderboard_hidden = false
  group by s.user_id, p.nickname, p.avatar_url
  order by sum(s.total_tokens) desc, s.user_id
  limit 200;
$$;

create or replace function get_leaderboard_date_grid(
  p_provider text,
  p_date_from date,
  p_date_to date,
  p_top_n integer default 10
) returns table (
  date date,
  rank integer,
  user_id uuid,
  nickname text,
  avatar_url text,
  total_tokens bigint
)
language sql
security definer
set search_path = public
as $$
  with per_date as (
    select
      s.date,
      s.user_id,
      sum(s.total_tokens)::bigint as total_tokens
    from daily_snapshots s
    join profiles p on p.id = s.user_id
    where s.provider = any(
            case when p_provider = 'combined'
                 then array['claude', 'codex']
                 else array[p_provider] end
          )
      and s.date >= p_date_from
      and s.date <= p_date_to
      and p.leaderboard_hidden = false
    group by s.date, s.user_id
  ),
  ranked as (
    select
      d.date,
      d.user_id,
      d.total_tokens,
      row_number() over (
        partition by d.date
        order by d.total_tokens desc, d.user_id
      )::integer as rank
    from per_date d
  )
  select
    r.date,
    r.rank,
    r.user_id,
    p.nickname,
    p.avatar_url,
    r.total_tokens
  from ranked r
  join profiles p on p.id = r.user_id
  where r.rank <= p_top_n
  order by r.date desc, r.rank asc;
$$;

create or replace function get_badge_rank(
  p_user_id uuid,
  p_provider text,
  p_date_from date,
  p_date_to date
) returns table (rank integer)
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
    where s.provider = any(
            case when p_provider = 'combined'
                 then array['claude', 'codex']
                 else array[p_provider] end
          )
      and s.date >= p_date_from
      and s.date <= p_date_to
      and p.leaderboard_hidden = false
    group by s.user_id
  ),
  ranked as (
    select
      t.user_id,
      row_number() over (order by t.total_tokens desc, t.user_id)::integer as rank
    from totals t
  )
  select r.rank
  from ranked r
  where r.user_id = p_user_id;
$$;
