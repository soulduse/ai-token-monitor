-- Add reply_to column to chat_messages for threading
ALTER TABLE chat_messages ADD COLUMN reply_to uuid REFERENCES chat_messages(id) ON DELETE SET NULL;
CREATE INDEX idx_chat_messages_reply_to ON chat_messages(reply_to);

-- Create chat_reactions table
CREATE TABLE chat_reactions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  message_id uuid REFERENCES chat_messages(id) ON DELETE CASCADE NOT NULL,
  user_id uuid REFERENCES profiles(id) NOT NULL,
  reaction_type text NOT NULL CHECK (reaction_type IN ('like', 'heart', 'dislike')),
  created_at timestamptz DEFAULT now(),
  CONSTRAINT unique_user_message_reaction UNIQUE (message_id, user_id, reaction_type)
);

CREATE INDEX idx_chat_reactions_message_id ON chat_reactions(message_id);
ALTER TABLE chat_reactions ENABLE ROW LEVEL SECURITY;

-- RLS: Read reactions (same criteria as chat_read)
CREATE POLICY "reactions_read" ON chat_reactions FOR SELECT USING (
  auth.uid() IS NOT NULL
  AND EXISTS (SELECT 1 FROM daily_snapshots WHERE user_id = auth.uid() LIMIT 1)
);

-- RLS: Insert own reactions only
CREATE POLICY "reactions_insert" ON chat_reactions FOR INSERT WITH CHECK (
  auth.uid() = user_id
  AND EXISTS (SELECT 1 FROM daily_snapshots WHERE user_id = auth.uid() LIMIT 1)
);

-- RLS: Delete own reactions (for toggle off)
CREATE POLICY "reactions_delete" ON chat_reactions FOR DELETE USING (auth.uid() = user_id);

-- Enable full replica identity for DELETE payload (needed for realtime old row data)
ALTER TABLE chat_reactions REPLICA IDENTITY FULL;

-- Enable Supabase Realtime for reactions
ALTER PUBLICATION supabase_realtime ADD TABLE chat_reactions;
