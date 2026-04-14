-- Shrink the Realtime broadcast payload for chat_reactions.
--
-- Previously `REPLICA IDENTITY FULL` caused the entire row (including every
-- column) to be written into WAL on UPDATE/DELETE, which Supabase Realtime
-- then broadcasts to every subscriber. Switching to `USING INDEX` makes WAL
-- only include the columns covered by the identity index, which is all the
-- fields the client actually needs for its delete/insert handlers
-- (message_id, user_id, reaction_type).
--
-- The same unique index also enforces "one reaction of a given type per user
-- per message", which was a latent gap — previously nothing stopped a client
-- from inserting duplicates.

-- Purge any pre-existing duplicates before adding the unique constraint.
delete from chat_reactions a
using chat_reactions b
where a.ctid < b.ctid
  and a.message_id    = b.message_id
  and a.user_id       = b.user_id
  and a.reaction_type = b.reaction_type;

create unique index if not exists chat_reactions_msg_user_type_uq
  on chat_reactions(message_id, user_id, reaction_type);

alter table chat_reactions replica identity using index chat_reactions_msg_user_type_uq;
