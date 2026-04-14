-- Schedule cleanup_old_chat_messages() via pg_cron.
--
-- The function itself was defined in 20260326_chat_messages.sql and updated
-- in 20260402_chat_images.sql to also purge storage objects, but the cron
-- schedule was never registered — so 7-day retention was never enforced.
-- Chat messages and chat-image storage objects have been accumulating since
-- 2026-03-26.

create extension if not exists pg_cron;

-- Unschedule any pre-existing job of the same name (idempotent re-runs)
do $$
begin
  if exists (select 1 from cron.job where jobname = 'cleanup-old-chat-messages') then
    perform cron.unschedule('cleanup-old-chat-messages');
  end if;
end $$;

-- Run hourly
select cron.schedule(
  'cleanup-old-chat-messages',
  '0 * * * *',
  $$ select cleanup_old_chat_messages(); $$
);

-- Immediate one-shot to clear the backlog accumulated since 2026-03-26
select cleanup_old_chat_messages();
