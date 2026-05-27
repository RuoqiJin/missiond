import fs from 'fs';
import net from 'net';
import os from 'os';
import path from 'path';
import { headers } from 'next/headers';

function resolveSocketPath(): string {
  if (process.env.MISSION_IPC_ENDPOINT) return process.env.MISSION_IPC_ENDPOINT;
  if (process.env.MISSION_IPC_SOCKET) return process.env.MISSION_IPC_SOCKET;
  const home = os.homedir();
  const newPath = path.join(home, '.missiond', 'missiond.sock');
  const legacyPath = path.join(home, '.xjp-mission', 'missiond.sock');
  if (fs.existsSync(newPath)) return newPath;
  if (fs.existsSync(legacyPath)) return legacyPath;
  return newPath;
}

const SOCKET_PATH = resolveSocketPath();

export type MissiondErrorBody = {
  code?: string;
  error_code?: string;
  message?: string;
  reason?: string;
  suggestion?: string;
  suggestedAction?: string;
  details?: unknown;
  [key: string]: unknown;
};

export class MissiondError extends Error {
  body: MissiondErrorBody;
  code?: string;

  constructor(body: MissiondErrorBody) {
    super(body.message ?? body.reason ?? body.error_code ?? body.code ?? 'MissionD error');
    this.name = 'MissiondError';
    this.body = body;
    this.code = body.code ?? body.error_code;
  }
}

function normalizeErrorBody(error: MissiondErrorBody): MissiondErrorBody {
  const code = error.code ?? error.error_code;
  const message = error.message ?? error.reason;
  const suggestedAction = error.suggestedAction ?? error.suggestion;
  return { ...error, code, message, suggestedAction };
}

export async function callMissiond(method: string, params: Record<string, unknown>): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(SOCKET_PATH, () => {
      const rpc = JSON.stringify({ jsonrpc: '2.0', id: 1, method, params });
      socket.write(rpc + '\n');
    });

    let data = '';
    socket.on('data', (chunk) => { data += chunk.toString(); });
    socket.on('end', () => {
      try {
        const resp = JSON.parse(data.trim());
        if (resp.error) {
          reject(new MissiondError(normalizeErrorBody(resp.error)));
        } else {
          resolve(resp.result);
        }
      } catch {
        reject(new Error(`Invalid response from missiond: ${data.slice(0, 200)}`));
      }
    });
    socket.on('error', (err) => reject(err));
    socket.setTimeout(10_000, () => {
      socket.destroy();
      reject(new Error('missiond IPC timeout'));
    });
  });
}

export async function callTool(name: string, args: Record<string, unknown> = {}): Promise<unknown> {
  // Reading headers() implicitly opts all callers into dynamic rendering,
  // preventing Next.js from caching API route responses at build time.
  headers();

  const result = await callMissiond('tools/call', { name, arguments: args }) as {
    content?: Array<{ text?: string }>;
    isError?: boolean;
    is_error?: boolean;
  };
  const text = result?.content?.[0]?.text;
  if (text) {
    const body = JSON.parse(text);
    if (result.isError || result.is_error) {
      throw new MissiondError(normalizeErrorBody(body));
    }
    return body;
  }
  return result;
}
