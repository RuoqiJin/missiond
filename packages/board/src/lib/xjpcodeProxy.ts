const DEFAULT_XJPCODE_BASE_URL = 'http://127.0.0.1:4040';

function envFlag(name: string): boolean {
  const value = process.env[name];
  return value === '1' || value === 'true' || value === 'TRUE' || value === 'yes' || value === 'YES';
}

function envBaseUrl(): string {
  return (
    process.env.MISSIOND_BOARD_XJPCODE_BASE_URL
    || process.env.MISSIOND_XJPCODE_BASE_URL
    || process.env.XJPCODE_BASE_URL
    || DEFAULT_XJPCODE_BASE_URL
  );
}

function isLoopbackHost(hostname: string): boolean {
  const host = hostname.trim().replace(/^\[|\]$/g, '').toLowerCase();
  return host === 'localhost'
    || host === '::1'
    || host === '127.0.0.1'
    || host.startsWith('127.');
}

export function resolveXjpcodeBaseUrl(raw?: unknown): string {
  const candidate = typeof raw === 'string' && raw.trim() ? raw.trim() : envBaseUrl();
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    throw new Error(`Invalid xjpcode base URL: ${candidate}`);
  }

  if (!['http:', 'https:'].includes(parsed.protocol)) {
    throw new Error('xjpcode base URL must use http or https');
  }

  const allowRemote = envFlag('MISSIOND_BOARD_XJPCODE_ALLOW_REMOTE');
  if (!allowRemote && !isLoopbackHost(parsed.hostname)) {
    throw new Error('xjpcode base URL must be loopback unless MISSIOND_BOARD_XJPCODE_ALLOW_REMOTE=1');
  }

  parsed.pathname = parsed.pathname.replace(/\/+$/, '');
  parsed.search = '';
  parsed.hash = '';
  return parsed.toString().replace(/\/$/, '');
}

export function xjpcodeUrl(baseUrl: string, path: string): string {
  const base = resolveXjpcodeBaseUrl(baseUrl);
  return `${base}${path.startsWith('/') ? path : `/${path}`}`;
}
