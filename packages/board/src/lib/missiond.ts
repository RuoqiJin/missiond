import net from 'net';
import os from 'os';
import path from 'path';

const SOCKET_PATH =
  process.env.MISSION_IPC_ENDPOINT ||
  process.env.MISSION_IPC_SOCKET ||
  path.join(os.homedir(), '.xjp-mission', 'missiond.sock');

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

export async function callTool(name: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const result = await callMissiond('tools/call', { name, arguments: args }) as {
    content?: Array<{ text?: string }>;
  };
  const text = result?.content?.[0]?.text;
  if (text) return JSON.parse(text);
  return result;
}
