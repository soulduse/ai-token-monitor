-- Add image support to chat messages

-- 1. Add image_url column
alter table chat_messages add column image_url text;

-- 2. Drop NOT NULL on content (image-only messages have empty content)
alter table chat_messages alter column content set default '';
alter table chat_messages alter column content drop not null;

-- 3. Update content constraint: allow empty content when image is present
alter table chat_messages drop constraint chat_messages_content_length;
alter table chat_messages add constraint chat_messages_content_check check (
  char_length(content) <= 500
  and (
    image_url is not null  -- image message: content can be empty
    or char_length(content) >= 1  -- text message: content required
  )
);

-- 3. Create storage bucket for chat images (public read)
insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values (
  'chat-images',
  'chat-images',
  true,
  2097152,  -- 2MB
  array['image/png', 'image/jpeg', 'image/gif', 'image/webp']
);

-- 4. Storage RLS: authenticated users can upload to their own folder
create policy "chat_images_upload" on storage.objects for insert to authenticated
with check (
  bucket_id = 'chat-images'
  and (storage.foldername(name))[1] = auth.uid()::text
);

-- 5. Storage RLS: public read (bucket is public, but explicit policy)
create policy "chat_images_read" on storage.objects for select
using (bucket_id = 'chat-images');

-- 6. Storage RLS: users can delete their own images
create policy "chat_images_delete" on storage.objects for delete to authenticated
using (
  bucket_id = 'chat-images'
  and (storage.foldername(name))[1] = auth.uid()::text
);

-- 7. Auto-delete storage image when chat message is deleted
-- Handles both manual deletion and 7-day cleanup cron
-- SECURITY DEFINER: needs to bypass storage RLS to delete any user's image
create or replace function cleanup_chat_image() returns trigger as $$
declare
  storage_path text;
begin
  if old.image_url is not null then
    storage_path := split_part(old.image_url, '/chat-images/', 2);
    if storage_path is not null and storage_path != '' then
      delete from storage.objects
      where bucket_id = 'chat-images' and name = storage_path;
    end if;
  end if;
  return old;
end;
$$ language plpgsql security definer;

create trigger chat_image_cleanup after delete on chat_messages
  for each row execute function cleanup_chat_image();
