#!/usr/bin/env node

import fs from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const usage = `Usage:
  node scripts/check-m6-deployment-status.mjs [--json] [--report-only] [--base-url URL]

Checks whether projects currently marked M6 in MissionD Universe also have
deploy-center evidence for a deployed production release.

This is intentionally separate from final convergence: M6 is SSOT/code maturity;
this script answers "is that M6 build deployed or at least deployment-confirmed?"
from deploy-center status/provenance evidence.
`;

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const DEFAULT_BASE_URL = process.env.DEPLOY_CENTER_PUBLIC_BASE_URL ?? 'https://auth.xiaojinpro.com';

const DEPLOYMENT_MAP = {
  auth: {
    slugs: ['xjp-auth-center'],
    repo: '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend',
    paths: ['services/auth', 'crates/xjp-auth-verifier'],
  },
  'deploy-center': {
    slugs: ['xjp-deploy-center'],
    repo: '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend',
    paths: ['services/deploy-center'],
  },
  router: {
    slugs: ['xjp-router'],
    repo: '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend',
    paths: ['services/router', 'crates/xjp-billing', 'crates/xjp-auth-verifier'],
  },
  pcea: {
    slugs: ['pcea', 'pcea-api', 'pcea-video-vault'],
    repo: '/Users/jinchen/Downloads/PCEA develop',
    paths: ['.missiond', 'pcea-api', 'pcea-video-vault'],
  },
};

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const m6Projects = readM6Projects(process.cwd());
  const deployStatus = await fetchDeployStatus(opts.baseUrl);
  const recentDeployments = deployStatus?.recent_deployments ?? [];
  const projects = m6Projects.map((projectId) => classifyProject(projectId, recentDeployments));
  const blocking = projects.filter((project) => !['deployed-current', 'no-deploy-target'].includes(project.status));
  const result = {
    ok: blocking.length === 0,
    checked_at: new Date().toISOString(),
    source: {
      maturity: BLUEPRINT,
      deploy_center_status: `${opts.baseUrl.replace(/\/$/, '')}/api/deploy/status`,
    },
    deploy_center_healthy: deployStatus?.healthy ?? null,
    m6_projects: m6Projects,
    projects,
    blocking_items: blocking.map((project) => ({
      project_id: project.project_id,
      status: project.status,
      reason: project.reason,
      deploy_slugs: project.deploy_slugs,
      latest_deploy: project.latest_deploy,
      changed_paths_since_deploy: project.changed_paths_since_deploy,
    })),
    recommended_deploy_order: recommendDeployOrder(projects),
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    printHuman(result);
  }
  process.exit(result.ok || opts.reportOnly ? 0 : 1);
}

function parseArgs(args) {
  const opts = { json: false, reportOnly: false, baseUrl: DEFAULT_BASE_URL };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--report-only') opts.reportOnly = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--base-url') {
      opts.baseUrl = args[++i] ?? fail('--base-url requires a value');
    } else if (arg.startsWith('--base-url=')) {
      opts.baseUrl = arg.slice('--base-url='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

function readM6Projects(root) {
  const source = fs.readFileSync(path.join(root, BLUEPRINT), 'utf8');
  const projects = [];
  const re = /\(maturity\s+:id\s+([a-zA-Z0-9_-]+)\s+:current\s+M6\b/g;
  let match;
  while ((match = re.exec(source))) projects.push(match[1]);
  return projects;
}

async function fetchDeployStatus(baseUrl) {
  const url = `${baseUrl.replace(/\/$/, '')}/api/deploy/status`;
  try {
    const res = await fetchJson(url);
    return res;
  } catch (err) {
    return { healthy: null, recent_deployments: [], error: err.message };
  }
}

function fetchJson(url) {
  return new Promise((resolve, reject) => {
    const req = https.get(url, { timeout: 8_000 }, (res) => {
      let body = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => {
        body += chunk;
      });
      res.on('end', () => {
        if (res.statusCode < 200 || res.statusCode >= 300) {
          reject(new Error(`HTTP ${res.statusCode} from ${url}`));
          return;
        }
        try {
          resolve(JSON.parse(body));
        } catch (err) {
          reject(new Error(`invalid JSON from ${url}: ${err.message}`));
        }
      });
    });
    req.on('timeout', () => {
      req.destroy(new Error(`timeout from ${url}`));
    });
    req.on('error', reject);
  });
}

function classifyProject(projectId, recentDeployments) {
  const map = DEPLOYMENT_MAP[projectId];
  if (!map) {
    return {
      project_id: projectId,
      status: 'no-deploy-target',
      reason: 'project is M6 but has no deploy-center production target mapping in MissionD deployment checker',
      deploy_slugs: [],
      latest_deploy: null,
      changed_paths_since_deploy: [],
    };
  }

  const latest = recentDeployments.find((row) => map.slugs.includes(row.project) && row.status === 'success') ?? null;
  if (!latest) {
    return {
      project_id: projectId,
      status: 'not-confirmed',
      reason: 'no successful recent deploy-center row matched the project deploy slugs',
      deploy_slugs: map.slugs,
      latest_deploy: null,
      changed_paths_since_deploy: [],
    };
  }

  const changed = changedPathsSinceDeploy(map.repo, latest.commit_hash, map.paths);
  if (changed.unknown) {
    return {
      project_id: projectId,
      status: 'deployed-unknown',
      reason: changed.reason,
      deploy_slugs: map.slugs,
      latest_deploy: latest,
      changed_paths_since_deploy: [],
    };
  }
  if (changed.paths.length > 0) {
    return {
      project_id: projectId,
      status: 'deployed-stale',
      reason: 'deploy-center has a successful deploy row, but local M6-relevant files changed after the deployed commit',
      deploy_slugs: map.slugs,
      latest_deploy: latest,
      changed_paths_since_deploy: changed.paths,
    };
  }
  return {
    project_id: projectId,
    status: 'deployed-current',
    reason: 'latest successful deploy-center row covers current M6-relevant files',
    deploy_slugs: map.slugs,
    latest_deploy: latest,
    changed_paths_since_deploy: [],
  };
}

function changedPathsSinceDeploy(repo, commitHash, paths) {
  if (!commitHash) return { unknown: true, reason: 'latest deploy row has no commit_hash', paths: [] };
  if (!fs.existsSync(path.join(repo, '.git'))) return { unknown: true, reason: `repo is not a git checkout: ${repo}`, paths: [] };
  const commit = resolveCommit(repo, commitHash);
  if (!commit) return { unknown: true, reason: `deployed commit is not present locally: ${commitHash}`, paths: [] };
  const proc = spawnSync('git', ['diff', '--name-only', `${commit}..HEAD`, '--', ...paths], {
    cwd: repo,
    encoding: 'utf8',
    timeout: 30_000,
  });
  if (proc.status !== 0 || proc.error) {
    return { unknown: true, reason: proc.error?.message ?? proc.stderr.trim() ?? 'git diff failed', paths: [] };
  }
  return { unknown: false, paths: proc.stdout.split(/\r?\n/).filter(Boolean) };
}

function resolveCommit(repo, commitHash) {
  const proc = spawnSync('git', ['rev-parse', '--verify', `${commitHash}^{commit}`], {
    cwd: repo,
    encoding: 'utf8',
    timeout: 30_000,
  });
  if (proc.status !== 0 || proc.error) return null;
  return proc.stdout.trim();
}

function recommendDeployOrder(projects) {
  const byId = Object.fromEntries(projects.map((project) => [project.project_id, project]));
  const order = [];
  if (needsDeploy(byId['deploy-center'])) order.push('deploy-center');
  if (needsDeploy(byId.router)) order.push('router');
  if (needsDeploy(byId.pcea)) order.push('pcea');
  if (needsDeploy(byId.auth)) order.push('auth');
  return order;
}

function needsDeploy(project) {
  return project && !['deployed-current', 'no-deploy-target'].includes(project.status);
}

function printHuman(result) {
  console.log(`M6 deployment status (${result.source.deploy_center_status})`);
  for (const project of result.projects) {
    console.log(`- ${project.project_id}: ${project.status} — ${project.reason}`);
  }
  if (result.recommended_deploy_order.length > 0) {
    console.log(`recommended deploy order: ${result.recommended_deploy_order.join(' -> ')}`);
  } else {
    console.log('recommended deploy order: none');
  }
}

main().catch((err) => {
  console.error(err.stack ?? err.message);
  process.exit(1);
});
