/**
 * Vite middleware: proxy /api/tasks → missiond IPC
 *
 * Ported from xiaojinpro-frontend/src/app/api/tasks/route.ts
 */

import net from 'net';
import os from 'os';
import path from 'path';
import type { IncomingMessage, ServerResponse } from 'http';

const SOCKET_PATH =
  process.env.MISSION_IPC_ENDPOINT ||
  process.env.MISSION_IPC_SOCKET ||
  path.join(os.homedir(), '.xjp-mission', 'missiond.sock');

async function callMissiond(method: string, params: Record<string, unknown>): Promise<unknown> {
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
          reject(new Error(resp.error.message || JSON.stringify(resp.error)));
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

async function callTool(name: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const result = await callMissiond('tools/call', { name, arguments: args }) as {
    content?: Array<{ text?: string }>;
  };
  const text = result?.content?.[0]?.text;
  if (text) return JSON.parse(text);
  return result;
}

function mapToFrontend(task: Record<string, unknown>): Record<string, unknown> {
  const { orderIdx, ...rest } = task;
  return { ...rest, order: orderIdx ?? 0 };
}

function mapToBackend(data: Record<string, unknown>): Record<string, unknown> {
  const { order, ...rest } = data;
  if (order !== undefined) rest.orderIdx = order;
  return rest;
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = '';
    req.on('data', (chunk: Buffer) => { body += chunk.toString(); });
    req.on('end', () => resolve(body));
  });
}

function json(res: ServerResponse, data: unknown, status = 200) {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(data));
}

async function handleGET(url: URL, res: ServerResponse) {
  const status = url.searchParams.get('status') || undefined;
  const args: Record<string, unknown> = {};
  if (status) args.status = status;

  const tasks = await callTool('mission_board_list', args) as Record<string, unknown>[];
  json(res, tasks.map(mapToFrontend));
}

async function handlePOST(url: URL, req: IncomingMessage, res: ServerResponse) {
  const action = url.searchParams.get('action');
  const id = url.searchParams.get('id');

  if (action === 'toggle' && id) {
    const result = await callTool('mission_board_toggle', { id });
    json(res, mapToFrontend(result as Record<string, unknown>));
    return;
  }

  if (action === 'clear-done') {
    const result = await callTool('mission_board_list', { status: 'done' }) as Record<string, unknown>[];
    for (const task of result) {
      await callTool('mission_board_delete', { id: task.id });
    }
    json(res, { deleted: result.length });
    return;
  }

  const body = JSON.parse(await readBody(req));
  const backendData = mapToBackend(body);
  const task = await callTool('mission_board_create', backendData);
  json(res, mapToFrontend(task as Record<string, unknown>));
}

async function handlePATCH(url: URL, req: IncomingMessage, res: ServerResponse) {
  const id = url.searchParams.get('id');
  if (!id) { json(res, { error: 'Missing id' }, 400); return; }

  const body = JSON.parse(await readBody(req));
  const backendData = mapToBackend(body);
  const task = await callTool('mission_board_update', { id, ...backendData });
  json(res, mapToFrontend(task as Record<string, unknown>));
}

async function handleDELETE(url: URL, res: ServerResponse) {
  const id = url.searchParams.get('id');
  if (!id) { json(res, { error: 'Missing id' }, 400); return; }

  const result = await callTool('mission_board_delete', { id });
  json(res, result);
}

export function createProxyMiddleware() {
  return async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    if (!req.url?.startsWith('/api/tasks')) return next();

    const url = new URL(req.url, `http://${req.headers.host || 'localhost'}`);

    try {
      switch (req.method) {
        case 'GET': await handleGET(url, res); break;
        case 'POST': await handlePOST(url, req, res); break;
        case 'PATCH': await handlePATCH(url, req, res); break;
        case 'DELETE': await handleDELETE(url, res); break;
        default: json(res, { error: 'Method not allowed' }, 405);
      }
    } catch (err) {
      console.error('[Board API] error:', err);
      json(res, { error: String(err) }, 502);
    }
  };
}
