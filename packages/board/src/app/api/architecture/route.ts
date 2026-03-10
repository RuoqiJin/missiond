import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

interface BeaconNode {
  beacon_name: string;
  repo: string;
  file_path: string;
  symbol_name: string;
  annotation?: string;
  signature?: string;
  stub_content?: string;
  start_line?: number;
  end_line?: number;
  node_type?: string;
}

interface CodeSearchHit {
  name: string;
  node_type: string;
  file_path: string;
  repo: string;
  lines: string;
  exported: boolean;
  signature: string;
  calls: string[];
  docstring?: string;
  stub?: string;
}

export async function GET(req: NextRequest) {
  try {
    const beacon = req.nextUrl.searchParams.get('beacon');
    const filePrefix = req.nextUrl.searchParams.get('file_prefix');
    const query = req.nextUrl.searchParams.get('query');

    let nodes: ArchNodeData[] = [];
    let edges: ArchEdgeData[] = [];

    if (beacon) {
      // Fetch beacon topology
      const result = await callTool('mission_beacon_map', { name: beacon }) as {
        beacon: string;
        node_count: number;
        nodes: BeaconNode[];
      };

      if (result?.nodes) {
        for (const n of result.nodes) {
          nodes.push({
            id: `${n.repo}:${n.file_path}:${n.symbol_name}:${n.node_type || 'function'}`,
            name: n.symbol_name,
            node_type: n.node_type || 'function',
            file_path: n.file_path,
            repo: n.repo,
            signature: n.signature || n.symbol_name,
            stub: n.stub_content || '',
            is_exported: true,
            beacon: beacon,
            start_line: n.start_line || 0,
            end_line: n.end_line || 0,
            annotation: n.annotation,
            lines: n.start_line && n.end_line ? n.end_line - n.start_line : 0,
          });
        }
      }
    }

    if (query || filePrefix) {
      // Fetch via code search
      const searchArgs: Record<string, unknown> = { limit: 100 };
      if (query) searchArgs.query = query;
      if (filePrefix) searchArgs.file_path = filePrefix;

      const result = await callTool('mission_code_search', searchArgs) as {
        results?: CodeSearchHit[];
      };

      if (result?.results) {
        for (const h of result.results) {
          const [startStr, endStr] = (h.lines || '0-0').split('-');
          const startLine = parseInt(startStr) || 0;
          const endLine = parseInt(endStr) || 0;
          const id = `${h.repo}:${h.file_path}:${h.name}:${h.node_type}`;

          // Deduplicate with beacon nodes
          if (!nodes.some(n => n.id === id)) {
            nodes.push({
              id,
              name: h.name,
              node_type: h.node_type,
              file_path: h.file_path,
              repo: h.repo,
              signature: h.signature,
              stub: h.stub || '',
              is_exported: h.exported,
              start_line: startLine,
              end_line: endLine,
              lines: endLine - startLine,
            });
          }

          // Build edges from calls
          if (h.calls?.length) {
            for (const callee of h.calls) {
              edges.push({
                source_name: h.name,
                target_name: callee,
                type: 'calls',
              });
            }
          }
        }
      }
    }

    // If only beacon was requested, also try to find call edges between beacon nodes
    if (beacon && !query && !filePrefix && nodes.length > 0) {
      // Search for each beacon node to get its calls
      const nodeNames = new Set(nodes.map(n => n.name));
      for (const node of nodes.slice(0, 30)) { // Limit to avoid too many searches
        try {
          const searchResult = await callTool('mission_code_search', {
            query: node.name,
            file_path: node.file_path.replace(/[^/]+$/, ''), // directory prefix
            limit: 5,
          }) as { results?: CodeSearchHit[] };

          if (searchResult?.results) {
            const match = searchResult.results.find(r =>
              r.name === node.name && r.file_path === node.file_path
            );
            if (match?.calls) {
              for (const callee of match.calls) {
                if (nodeNames.has(callee) && callee !== node.name) {
                  edges.push({
                    source_name: node.name,
                    target_name: callee,
                    type: 'calls',
                  });
                }
              }
            }
          }
        } catch { /* ignore individual search failures */ }
      }
    }

    // Resolve edges: match names to node IDs
    const nameToIds = new Map<string, string[]>();
    for (const n of nodes) {
      const existing = nameToIds.get(n.name) || [];
      existing.push(n.id);
      nameToIds.set(n.name, existing);
    }

    const resolvedEdges: { id: string; source: string; target: string; type: string; ambiguous: boolean }[] = [];
    const edgeSet = new Set<string>();

    for (const e of edges) {
      const sources = nameToIds.get(e.source_name) || [];
      const targets = nameToIds.get(e.target_name) || [];
      if (sources.length === 0 || targets.length === 0) continue;

      const ambiguous = targets.length > 1;
      for (const src of sources) {
        for (const tgt of targets) {
          if (src === tgt) continue;
          const edgeId = `${src}->${tgt}`;
          if (edgeSet.has(edgeId)) continue;
          edgeSet.add(edgeId);
          resolvedEdges.push({ id: edgeId, source: src, target: tgt, type: e.type, ambiguous });
        }
      }
    }

    // Group nodes by file for the frontend
    const fileGroups = new Map<string, typeof nodes>();
    for (const n of nodes) {
      const key = n.file_path;
      const group = fileGroups.get(key) || [];
      group.push(n);
      fileGroups.set(key, group);
    }

    return NextResponse.json({
      nodes,
      edges: resolvedEdges,
      files: Array.from(fileGroups.keys()),
      node_count: nodes.length,
      edge_count: resolvedEdges.length,
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

interface ArchNodeData {
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

interface ArchEdgeData {
  source_name: string;
  target_name: string;
  type: string;
}
