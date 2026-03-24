alter table daily_snapshots
add column if not exists provider text not null default 'claude';

alter table daily_snapshots
drop constraint if exists daily_snapshots_user_id_date_key;

alter table daily_snapshots
drop constraint if exists daily_snapshots_provider_check;

alter table daily_snapshots
add constraint daily_snapshots_provider_check
check (provider in ('claude', 'codex'));

alter table daily_snapshots
add constraint daily_snapshots_user_id_date_provider_key
unique (user_id, date, provider);

create index if not exists idx_snapshots_date_provider
on daily_snapshots(date, provider);

create or replace function public.get_leaderboard(
  p_start_date date,
  p_end_date date,
  p_providers text[]
)
returns table (
  user_id uuid,
  nickname text,
  avatar_url text,
  total_tokens bigint,
  cost_usd numeric,
  messages bigint,
  sessions bigint
)
language sql
stable
set search_path = public
as $$
  select
    ds.user_id,
    p.nickname,
    p.avatar_url,
    sum(ds.total_tokens)::bigint as total_tokens,
    sum(ds.cost_usd) as cost_usd,
    sum(ds.messages)::bigint as messages,
    sum(ds.sessions)::bigint as sessions
  from daily_snapshots ds
  join profiles p on p.id = ds.user_id
  where ds.date >= p_start_date
    and ds.date <= p_end_date
    and ds.provider = any(p_providers)
  group by ds.user_id, p.nickname, p.avatar_url
  order by total_tokens desc
  limit 100;
$$;

grant execute on function public.get_leaderboard(date, date, text[]) to anon;
grant execute on function public.get_leaderboard(date, date, text[]) to authenticated;
