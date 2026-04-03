-- Add links column to profiles (max 3 personal links with metadata)
alter table profiles
add column if not exists links jsonb not null default '[]'::jsonb;
