#!/usr/bin/env node
import { spawn } from 'node:child_process';

const cliArgs = process.argv.slice(2);
const payloadOnly =
  cliArgs.includes('--payload') || process.env.MISSION_MCP_CALL_PAYLOAD === '1';
const positionalArgs = cliArgs.filter((arg) => arg !== '--payload');
const [toolName, rawArgs = '{}'] = positionalArgs;
if (!toolName) {
  console.error('Usage: node scripts/mission-mcp-call.mjs [--payload] <tool-name> <json-args>');
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
  process.stderr.write(`mission-mcp-call timed out calling ${toolName}\n`);
  if (stderr.trim()) process.stderr.write(`${stderr.trim()}\n`);
  process.exit(124);
}, Number(process.env.MISSION_MCP_CALL_TIMEOUT_MS ?? 120000));

child.on('close', (code) => {
  void finish(code);
});

async function finish(code) {
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
    await writeStream(process.stderr, `${stderr.trim()}\n`);
  }
  if (callResponse) {
    if (payloadOnly) {
      const payload = extractToolPayload(callResponse);
      const output =
        typeof payload === 'string' ? payload : JSON.stringify(payload, null, 2);
      await writeStream(process.stdout, `${output}\n`);
    } else {
      await writeStream(process.stdout, `${JSON.stringify(callResponse, null, 2)}\n`);
    }
  }
  process.exitCode = code ?? 0;
}

function extractToolPayload(response) {
  const content = response?.result?.content;
  const textItem = Array.isArray(content)
    ? content.find((item) => typeof item?.text === 'string')
    : null;
  if (!textItem) return response;
  try {
    return JSON.parse(textItem.text);
  } catch {
    return textItem.text;
  }
}

function writeStream(stream, text) {
  return new Promise((resolve, reject) => {
    stream.write(text, (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}

for (const request of requests) {
  child.stdin.write(JSON.stringify(request));
  child.stdin.write('\n');
}
child.stdin.end();
