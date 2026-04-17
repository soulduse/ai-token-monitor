-- Relax cleanup_old_chat_messages() cron from hourly to daily.
--
-- Rationale: Nano instance Disk IO budget (30-min daily burst) has been
-- exhausted every day, degrading all queries to baseline 43 Mbps. Hourly
-- DELETE + storage.delete_object() across chat_messages and chat-images
-- triggers heavy WAL write + VACUUM IO.
--
-- 7-day retention still holds — a single daily sweep covers the same
-- delete volume in one batch during off-peak hours instead of spiking
-- every hour.

-- Drop the hourly schedule if it exists (idempotent)
do $$
begin
  if exists (select 1 from cron.job where jobname = 'cleanup-old-chat-messages') then
    perform cron.unschedule('cleanup-old-chat-messages');
  end if;
end $$;

-- Run once daily at 18:00 UTC (03:00 KST) — off-peak for Korean users
select cron.schedule(
  'cleanup-old-chat-messages',
  '0 18 * * *',
  $$ select cleanup_old_chat_messages(); $$
);
