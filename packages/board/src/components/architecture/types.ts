export interface ArchNodeData {
  id: string;
  name: string;
  node_type: string;
  file_path: string;
  repo: string;
  signature: string;
  stub: string;
  is_exported: boolean;
  beacon?: string;
  start_line: number;
  end_line: number;
  annotation?: string;
  lines: number;
}

export interface ArchEdge {
  id: string;
  source: string;
  target: string;
  type: string;
  ambiguous: boolean;
}

export interface GraphResponse {
  nodes: ArchNodeData[];
  edges: ArchEdge[];
  files: string[];
  node_count: number;
  edge_count: number;
}

// Beacon colors — 13 subsystems
export const BEACON_COLORS: Record<string, string> = {
  orchestration: '#3b82f6', // blue
  slot: '#f59e0b',          // amber
  knowledge: '#a855f7',     // purple
  holographic: '#06b6d4',   // cyan
  timeline: '#ec4899',      // pink
  decision: '#f97316',      // orange
  memory: '#22c55e',        // green
  pty: '#64748b',           // slate
  board: '#eab308',         // yellow
  router: '#ef4444',        // red
  eventbus: '#14b8a6',      // teal
  mcp: '#8b5cf6',           // violet
  infrastructure: '#78716c', // stone
};

export function getBeaconColor(beacon?: string): string {
  if (!beacon) return '#6b7280'; // gray default
  // Match partial beacon names (e.g., "orchestration" matches "orchestration/slot")
  for (const [key, color] of Object.entries(BEACON_COLORS)) {
    if (beacon.includes(key)) return color;
  }
  return '#6b7280';
}

// Shorten file path for display
export function shortPath(filePath: string): string {
  // "crates/missiond-daemon/src/autopilot.rs" → "daemon/autopilot.rs"
  const parts = filePath.split('/');
  const srcIdx = parts.indexOf('src');
  if (srcIdx > 0) {
    const crate = parts[srcIdx - 1].replace('missiond-', '');
    const subPath = parts.slice(srcIdx + 1).join('/');
    return `${crate}/${subPath}`;
  }
  return parts.slice(-2).join('/');
}
