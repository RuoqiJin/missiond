-- Backfill project_id for historical conversations using path prefix matching.
-- This is a one-time migration that resolves project from the existing `project` (cwd) column.
-- After this, new conversations get project_id set at ingestion time via ProjectRegistry.

-- Common project path prefixes → project_id mapping.
-- Uses longest-prefix match: UPDATE only rows that start with the given path.
-- Each UPDATE is idempotent (only sets if project_id IS NULL).

DO $$
DECLARE
    mappings TEXT[][] := ARRAY[
        ['<REPO_ROOT>', 'missiond'],
        ['<REPO_ROOT>/../example-forge', 'example-forge'],
        ['<REPO_ROOT>/../example-mechanic', 'example-mechanic'],
        ['<REPO_ROOT>/../jarvis', 'jarvis'],
        ['<REPO_ROOT>/../deploy-agent', 'example-deploy'],
        ['<PROJECTS_ROOT>/example-b', 'example-b'],
        ['<PROJECTS_ROOT>/example-c', 'example-backend'],
        ['<PROJECTS_ROOT>/example-d', 'example-editor'],
        ['<PROJECTS_ROOT>/example-e', 'example-cut']
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
    ('missiond', '<REPO_ROOT>', '.missiond/intent.lisp', true, '{}'),
    ('example-forge', '<REPO_ROOT>/../example-forge', '.jarvis/intent.lisp', true, '{lisp-surveyor}'),
    ('jarvis', '<REPO_ROOT>/../jarvis', NULL, true, '{}'),
    ('example-mechanic', '<REPO_ROOT>/../example-mechanic', NULL, false, '{}'),
    ('example-deploy', '<REPO_ROOT>/../deploy-agent', NULL, true, '{}'),
    ('example-b', '<PROJECTS_ROOT>/example-b', NULL, true, '{}'),
    ('example-backend', '<PROJECTS_ROOT>/example-c', NULL, true, '{}'),
    ('example-editor', '<PROJECTS_ROOT>/example-d', NULL, true, '{}'),
    ('example-cut', '<PROJECTS_ROOT>/example-e/example-cut', NULL, false, '{}')
ON CONFLICT (id) DO NOTHING;
