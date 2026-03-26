-- Chat messages table for leaderboard community chat
create table chat_messages (
  id uuid primary key default gen_random_uuid(),
  user_id uuid references profiles(id) not null,
  content text not null,
  created_at timestamptz default now(),
  constraint chat_messages_content_length check (char_length(content) between 1 and 500)
);

create index idx_chat_messages_created_at on chat_messages(created_at desc);
alter table chat_messages enable row level security;

-- RLS: Only leaderboard participants (users with at least one snapshot) can read
create policy "chat_read" on chat_messages for select using (
  auth.uid() is not null
  and exists (select 1 from daily_snapshots where user_id = auth.uid() limit 1)
);

-- RLS: Only leaderboard participants can insert their own messages
create policy "chat_insert" on chat_messages for insert with check (
  auth.uid() = user_id
  and exists (select 1 from daily_snapshots where user_id = auth.uid() limit 1)
);

-- RLS: Users can delete their own messages
create policy "chat_delete" on chat_messages for delete using (auth.uid() = user_id);

-- Rate limiting: max 5 messages per 30 seconds per user
-- SECURITY DEFINER is intentional: count must bypass RLS to see all user's messages
create or replace function check_chat_rate_limit() returns trigger as $$
begin
  if (select count(*) from chat_messages
      where user_id = new.user_id and created_at > now() - interval '30 seconds') >= 5 then
    raise exception 'Rate limit exceeded';
  end if;
  return new;
end;
$$ language plpgsql security definer;

create trigger chat_rate_limit before insert on chat_messages
  for each row execute function check_chat_rate_limit();

-- Cleanup function for messages older than 7 days (schedule via pg_cron)
create or replace function cleanup_old_chat_messages() returns void as $$
begin
  delete from chat_messages where created_at < now() - interval '7 days';
end;
$$ language plpgsql security definer;

-- Enable Supabase Realtime for chat_messages
alter publication supabase_realtime add table chat_messages;
