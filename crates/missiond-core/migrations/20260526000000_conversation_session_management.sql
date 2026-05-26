-- Multi-tenant conversation session isolation, topic threading, and context capsule binding.
-- SSOT: .missiond/v3/shards/memory-knowledge-runtime.lisp :: conversation-session-management

-- Isolation dimensions (Auth-derived)
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS user_id TEXT;
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS tenant_id TEXT;
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS application_id TEXT;
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS channel TEXT DEFAULT 'cli';

-- Topic threading
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS topic_id TEXT;
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS topic_label TEXT;

-- Context capsule binding
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS context_capsule_hash TEXT;

-- Isolation query indexes
CREATE INDEX IF NOT EXISTS idx_conv_user_id ON conversations(user_id) WHERE user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conv_tenant_id ON conversations(tenant_id) WHERE tenant_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conv_application_id ON conversations(application_id) WHERE application_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conv_channel ON conversations(channel);

-- Topic threading indexes
CREATE INDEX IF NOT EXISTS idx_conv_topic_id ON conversations(topic_id) WHERE topic_id IS NOT NULL;

-- Composite isolation index for session resolution
CREATE INDEX IF NOT EXISTS idx_conv_session_resolve
    ON conversations(user_id, tenant_id, application_id, channel, topic_id)
    WHERE status = 'active';

-- Context capsule lookup
CREATE INDEX IF NOT EXISTS idx_conv_context_capsule ON conversations(context_capsule_hash) WHERE context_capsule_hash IS NOT NULL;
