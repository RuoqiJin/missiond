ALTER TABLE projects ADD COLUMN IF NOT EXISTS github_url TEXT;

-- Backfill from known repos
UPDATE projects SET github_url = 'https://github.com/RuoqiJin/missiond' WHERE id = 'missiond' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/example/example-forge' WHERE id = 'example-forge' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/example/jarvis' WHERE id = 'jarvis' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/example/deploy-agent' WHERE id = 'example-deploy' AND github_url IS NULL;
