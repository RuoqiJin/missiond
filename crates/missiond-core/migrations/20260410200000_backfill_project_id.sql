-- Backfill project_id for historical conversations using path prefix matching.
-- This is a one-time migration that resolves project from the existing `project` (cwd) column.
-- After this, new conversations get project_id set at ingestion time via ProjectRegistry.

-- Common project path prefixes → project_id mapping.
-- Uses longest-prefix match: UPDATE only rows that start with the given path.
-- Each UPDATE is idempotent (only sets if project_id IS NULL).

DO $$
DECLARE
    mappings TEXT[][] := ARRAY[
        ['/Users/jinchen/Projects/missiond', 'missiond'],
        ['/Users/jinchen/Projects/jarvis-forge', 'jarvis-forge'],
        ['/Users/jinchen/Projects/jarvis-mechanic', 'jarvis-mechanic'],
        ['/Users/jinchen/Projects/jarvis', 'jarvis'],
        ['/Users/jinchen/Projects/xjp-deploy-agent', 'xjp-deploy-agent'],
        ['/Users/jinchen/Downloads/PCEA/develop/pcea-video-vault', 'pcea-video-vault'],
        ['/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend', 'xiaojinpro-backend'],
        ['/Users/jinchen/development/SRTeditor', 'srteditor'],
        ['/Users/jinchen/development/xiaojincut', 'xiaojincut']
    ];
    m TEXT[];
    affected BIGINT;
    total BIGINT := 0;
BEGIN
    FOREACH m SLICE 1 IN ARRAY mappings LOOP
        UPDATE conversations
        SET project_id = m[2]
        WHERE project_id IS NULL
          AND project IS NOT NULL
          AND project LIKE m[1] || '%';
        GET DIAGNOSTICS affected = ROW_COUNT;
        IF affected > 0 THEN
            RAISE NOTICE 'Backfilled % conversations for project %', affected, m[2];
            total := total + affected;
        END IF;
    END LOOP;
    RAISE NOTICE 'Total conversations backfilled: %', total;
END $$;

-- Also seed the projects table with known projects (idempotent).
INSERT INTO projects (id, path, intent_path, active, slots)
VALUES
    ('missiond', '/Users/jinchen/Projects/missiond', '.missiond/intent.lisp', true, '{}'),
    ('jarvis-forge', '/Users/jinchen/Projects/jarvis-forge', '.jarvis/intent.lisp', true, '{lisp-surveyor}'),
    ('jarvis', '/Users/jinchen/Projects/jarvis', NULL, true, '{}'),
    ('jarvis-mechanic', '/Users/jinchen/Projects/jarvis-mechanic', NULL, false, '{}'),
    ('xjp-deploy-agent', '/Users/jinchen/Projects/xjp-deploy-agent', NULL, true, '{}'),
    ('pcea-video-vault', '/Users/jinchen/Downloads/PCEA/develop/pcea-video-vault', NULL, true, '{}'),
    ('xiaojinpro-backend', '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend', NULL, true, '{}'),
    ('srteditor', '/Users/jinchen/development/SRTeditor', NULL, true, '{}'),
    ('xiaojincut', '/Users/jinchen/development/xiaojincut/xiaojincut', NULL, false, '{}')
ON CONFLICT (id) DO NOTHING;
