#!/usr/bin/env node

import fs from 'node:fs';
import { spawnSync } from 'node:child_process';

const ROOT = process.cwd();

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  const compile = spawnSync(process.execPath, ['scripts/compile-v3-runtime.mjs', '--check', '--json'], {
    cwd: ROOT,
    encoding: 'utf8',
  });
  if (compile.status !== 0) {
    diagnostics.push({
      file: 'scripts/compile-v3-runtime.mjs',
      message: `compile-v3-runtime --check failed: ${compile.stderr || compile.stdout}`,
    });
    return finish(diagnostics, opts);
  }
  const result = JSON.parse(compile.stdout);
  const universePath = (result.results ?? []).find((row) => row.id === 'universe')?.path;
  if (!universePath || !fs.existsSync(universePath)) {
    diagnostics.push({ file: 'compiled-project-universe.json', message: 'compiled universe missing' });
    return finish(diagnostics, opts);
  }
  const universe = JSON.parse(fs.readFileSync(universePath, 'utf8'));
  checkUniverse(universe, diagnostics);
  finish(diagnostics, opts);
}

function checkUniverse(universe, diagnostics) {
  const payload = universe.payload ?? {};
  const services = Array.isArray(payload.services) ? payload.services : [];
  const auth = services.find((service) => service.id === 'auth');
  if (!auth) {
    diagnostics.push({ file: 'compiled-project-universe.json', message: 'auth service missing' });
  } else {
    if (auth.canonical_domain !== 'auth.xiaojinpro.com') {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: 'auth',
        message: `auth canonical_domain must remain auth.xiaojinpro.com, got ${auth.canonical_domain ?? '<missing>'}`,
      });
    }
    if ((auth.domains ?? []).includes('auth.xiaojins.com')) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: 'auth',
        message: 'auth.xiaojins.com must not be generated because WeChat callback uses auth.xiaojinpro.com',
      });
    }
    if (!auth.domain_exception_reason) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: 'auth',
        message: 'auth domain_exception_reason is required',
      });
    }
  }

  for (const service of services) {
    if (service.environment !== 'production') continue;
    const proxy = service.proxy;
    if (!proxy || proxy.kind !== 'caddy') continue;
    if (!service.canonical_domain) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${service.id} production Caddy service must expose canonical_domain`,
      });
    }
    if (!proxy.upstream && service.id !== 'auth') {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${service.id} production Caddy service must expose proxy.upstream`,
      });
    }
    if (!service.domain_binding_lifecycle?.active_requires?.includes('smoke_ready')) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${service.id} domain lifecycle must require smoke_ready before active`,
      });
    }
  }
  checkDuplicateCanonicalUpstreams(services, diagnostics);

  const managed = new Map((payload.domain_management?.domains ?? []).map((row) => [row.domain, row]));
  for (const service of services) {
    const domain = service.canonical_domain;
    if (!domain || !domain.endsWith('.xiaojins.com')) continue;
    if (!managed.has(domain)) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${domain} missing from domain_management`,
      });
      continue;
    }
    if (service.domain_proxy_intent && !managed.get(domain).proxy_intent) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${domain} domain_management row missing proxy_intent`,
      });
    }
  }

  const bundle = renderCaddyBundle(services);
  if (bundle.includes('auth.xiaojins.com')) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: 'generated Caddy bundle must not contain auth.xiaojins.com',
    });
  }
  if (!bundle.includes('code.xiaojins.com') || !bundle.includes('skills.xiaojins.com')) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: 'generated Caddy bundle must include code.xiaojins.com and skills.xiaojins.com',
    });
  }
}

function checkDuplicateCanonicalUpstreams(services, diagnostics) {
  const byUpstream = new Map();
  for (const service of services) {
    if (service.environment !== 'production') continue;
    if (!service.canonical_domain?.endsWith('.xiaojins.com')) continue;
    const intent = service.domain_proxy_intent;
    if (!intent?.domain || !intent?.upstream) continue;
    if (service.id === 'auth') continue;
    const rows = byUpstream.get(intent.upstream) ?? [];
    rows.push({ id: service.id, domain: intent.domain });
    byUpstream.set(intent.upstream, rows);
  }
  for (const [upstream, rows] of byUpstream.entries()) {
    if (rows.length <= 1) continue;
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: `proxy upstream ${upstream} is shared by canonical services: ${rows
        .map((row) => `${row.id}/${row.domain}`)
        .join(', ')}`,
    });
  }
}

function renderCaddyBundle(services) {
  return services
    .filter((service) => service.domain_proxy_intent?.domain && service.domain_proxy_intent?.upstream)
    .sort((a, b) => a.domain_proxy_intent.domain.localeCompare(b.domain_proxy_intent.domain))
    .map((service) => {
      const intent = service.domain_proxy_intent;
      const matcher = String(service.id).replace(/[^A-Za-z0-9]/g, '_');
      const routes = (intent.routes ?? []).join(' ');
      return `${intent.domain} {\n  @${matcher} path ${routes}\n  handle @${matcher} {\n    reverse_proxy ${intent.upstream}\n  }\n}\n`;
    })
    .join('\n');
}

function parseArgs(args) {
  return { json: args.includes('--json') };
}

function finish(diagnostics, opts) {
  const ok = diagnostics.length === 0;
  if (opts.json) {
    console.log(JSON.stringify({ ok, diagnostics }, null, 2));
  } else if (ok) {
    console.log('domain/proxy isomorphism OK');
  } else {
    for (const diagnostic of diagnostics) {
      console.error(`${diagnostic.file}: ${diagnostic.message}`);
    }
  }
  process.exit(ok ? 0 : 1);
}

main();
