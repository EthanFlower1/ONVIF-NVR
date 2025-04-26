-- Add segment_id and parent_recording_id columns to recordings table
-- Migration for upgrading existing databases

-- Add the new columns 
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS segment_id INTEGER;
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS parent_recording_id UUID REFERENCES recordings(id) ON DELETE CASCADE;

-- Create indexes for the new columns
CREATE INDEX IF NOT EXISTS idx_recordings_parent_id ON recordings(parent_recording_id);
CREATE INDEX IF NOT EXISTS idx_recordings_segment_id ON recordings(parent_recording_id, segment_id);

-- Migrate existing segment data from metadata to dedicated columns
-- This will extract segment_index from metadata and move it to segment_id
UPDATE recordings 
SET 
    segment_id = (metadata->>'segment_index')::INTEGER,
    parent_recording_id = (metadata->>'parent_recording_id')::UUID
WHERE 
    metadata->>'segment_index' IS NOT NULL 
    AND metadata->>'parent_recording_id' IS NOT NULL;

-- Update metadata to remove now-redundant fields
UPDATE recordings
SET metadata = metadata - 'segment_index' - 'parent_recording_id'
WHERE segment_id IS NOT NULL AND parent_recording_id IS NOT NULL;

-- Log an update message
DO $$
BEGIN
    RAISE NOTICE 'Migration complete: Added segment_id and parent_recording_id columns';
END $$;