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

-- 7. Update cleanup function to handle image storage deletion
-- Supabase blocks direct DELETE on storage.objects; use storage.delete_object() instead
-- All messages: 7 days retention
create or replace function cleanup_old_chat_messages() returns void as $$
declare
  r record;
  v_path text;
begin
  -- Delete storage images for image messages older than 7 days
  for r in
    select image_url from chat_messages
    where created_at < now() - interval '7 days'
    and image_url is not null
  loop
    v_path := split_part(r.image_url, '/chat-images/', 2);
    if v_path is not null and v_path != '' then
      begin
        perform storage.delete_object('chat-images', v_path);
      exception when others then
        null; -- skip if already deleted
      end;
    end if;
  end loop;

  -- Delete all messages older than 7 days
  delete from chat_messages where created_at < now() - interval '7 days';
end;
$$ language plpgsql security definer;
