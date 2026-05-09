#!/usr/bin/env node
import { spawn } from 'node:child_process';

const [, , toolName, rawArgs = '{}'] = process.argv;
if (!toolName) {
  console.error('Usage: node scripts/mission-mcp-call.mjs <tool-name> <json-args>');
  process.exit(2);
}

let args;
try {
  args = JSON.parse(rawArgs);
} catch (error) {
  console.error(`Invalid JSON args: ${error.message}`);
  process.exit(2);
}

const bin = process.env.MISSION_MCP_BIN ?? `${process.env.HOME}/.xjp-mission/mission-mcp`;
const child = spawn(bin, [], {
  stdio: ['pipe', 'pipe', 'pipe'],
  env: {
    ...process.env,
    MISSIOND_MCP_PRELOAD_INSTRUCTIONS: '0',
    MISSION_LOG_LEVEL: process.env.MISSION_LOG_LEVEL ?? 'error',
  },
});

const requests = [
  {
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'mission-mcp-call', version: '0.1.0' },
    },
  },
  { jsonrpc: '2.0', method: 'notifications/initialized' },
  {
    jsonrpc: '2.0',
    id: 2,
    method: 'tools/call',
    params: { name: toolName, arguments: args },
  },
];

let stdout = '';
let stderr = '';
child.stdout.on('data', (chunk) => {
  stdout += chunk.toString('utf8');
});
child.stderr.on('data', (chunk) => {
  stderr += chunk.toString('utf8');
});

const timeout = setTimeout(() => {
  child.kill('SIGTERM');
  console.error(`mission-mcp-call timed out calling ${toolName}`);
  if (stderr.trim()) console.error(stderr.trim());
  process.exit(124);
}, Number(process.env.MISSION_MCP_CALL_TIMEOUT_MS ?? 120000));

child.on('close', (code) => {
  clearTimeout(timeout);
  const lines = stdout.trim().split(/\n+/).filter(Boolean);
  const responses = [];
  for (const line of lines) {
    try {
      responses.push(JSON.parse(line));
    } catch {
      // Keep raw noise visible if any.
      responses.push({ raw: line });
    }
  }
  const callResponse = responses.find((response) => response.id === 2) ?? responses.at(-1);
  if (stderr.trim()) {
    console.error(stderr.trim());
  }
  if (callResponse) {
    console.log(JSON.stringify(callResponse, null, 2));
  }
  process.exit(code ?? 0);
});

for (const request of requests) {
  child.stdin.write(JSON.stringify(request));
  child.stdin.write('\n');
}
child.stdin.end();
