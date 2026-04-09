ALTER TABLE projects ADD COLUMN IF NOT EXISTS github_url TEXT;

-- Backfill from known repos
UPDATE projects SET github_url = 'https://github.com/RuoqiJin/missiond' WHERE id = 'missiond' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/xiaojinpro/jarvis-forge' WHERE id = 'jarvis-forge' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/xiaojinpro/jarvis' WHERE id = 'jarvis' AND github_url IS NULL;
UPDATE projects SET github_url = 'https://github.com/xiaojinpro/xjp-deploy-agent' WHERE id = 'xjp-deploy-agent' AND github_url IS NULL;
