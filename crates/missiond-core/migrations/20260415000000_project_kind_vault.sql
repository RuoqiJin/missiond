-- Add project kind (managed/reference), vault path, and parent_id for monorepo sub-services
ALTER TABLE projects ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'managed';
ALTER TABLE projects ADD COLUMN IF NOT EXISTS vault_path TEXT;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS parent_id TEXT;
