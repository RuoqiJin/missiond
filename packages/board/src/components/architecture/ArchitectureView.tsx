'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  BackgroundVariant,
  Panel,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import ELK, { type ElkNode } from 'elkjs/lib/elk.bundled.js';
import { Search, RefreshCw, Camera, Filter } from 'lucide-react';
import CodeNode from './CodeNode';
import { type GraphResponse, type ArchNodeData, getBeaconColor, shortPath, BEACON_COLORS } from './types';

const elk = new ELK();

const nodeTypes = { codeNode: CodeNode };

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyNode = Node<any>;

async function computeLayout(
  nodes: AnyNode[],
  edges: Edge[],
  fileGroups: Map<string, string[]>,
): Promise<{ nodes: AnyNode[]; edges: Edge[] }> {
  const children: ElkNode[] = [];
  const elkEdges: { id: string; sources: string[]; targets: string[] }[] = [];

  for (const [filePath, nodeIds] of fileGroups) {
    const groupId = `group-${filePath}`;
    children.push({
      id: groupId,
      labels: [{ text: shortPath(filePath) }],
      layoutOptions: {
        'elk.padding': '[top=40,left=10,bottom=10,right=10]',
      },
      children: nodeIds.map(nid => ({
        id: nid,
        width: 220,
        height: 80,
      })),
    });
  }

  for (const e of edges) {
    elkEdges.push({
      id: e.id,
      sources: [e.source],
      targets: [e.target],
    });
  }

  const elkGraph: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'DOWN',
      'elk.spacing.nodeNode': '40',
      'elk.layered.spacing.nodeNodeBetweenLayers': '60',
      'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
    },
    children,
    edges: elkEdges,
  };

  const layout = await elk.layout(elkGraph);

  const positionMap = new Map<string, { x: number; y: number }>();
  const groupNodes: AnyNode[] = [];

  if (layout.children) {
    for (const group of layout.children) {
      const gx = group.x || 0;
      const gy = group.y || 0;
      const filePath = group.id.replace('group-', '');
      groupNodes.push({
        id: group.id,
        type: 'group',
        position: { x: gx, y: gy },
        data: { label: shortPath(filePath) },
        style: {
          width: group.width || 260,
          height: group.height || 200,
          backgroundColor: 'rgba(30, 30, 30, 0.5)',
          borderColor: 'rgba(100, 100, 100, 0.3)',
          borderWidth: 1,
          borderRadius: 8,
          padding: 10,
        },
      });

      if (group.children) {
        for (const child of group.children) {
          positionMap.set(child.id, {
            x: child.x || 0,
            y: child.y || 0,
          });
        }
      }
    }
  }

  const positionedNodes: AnyNode[] = [
    ...groupNodes,
    ...nodes.map(n => {
      const pos = positionMap.get(n.id);
      const d = n.data as Record<string, unknown>;
      const filePath = d.file_path as string;
      const parentId = `group-${filePath}`;
      return {
        ...n,
        position: pos || { x: 0, y: 0 },
        parentId,
        extent: 'parent' as const,
      };
    }),
  ];

  return { nodes: positionedNodes, edges };
}

export default function ArchitectureView() {
  const [nodes, setNodes, onNodesChange] = useNodesState<AnyNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [beacons, setBeacons] = useState<string[]>([]);
  const [selectedBeacon, setSelectedBeacon] = useState<string>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [graphData, setGraphData] = useState<GraphResponse | null>(null);

  // Fetch available beacons
  useEffect(() => {
    fetch('/api/architecture/beacons')
      .then(r => r.json())
      .then((data: { name: string }[]) => {
        if (Array.isArray(data)) {
          setBeacons(data.map(b => b.name));
        }
      })
      .catch(() => {});
  }, []);

  const fetchGraph = useCallback(async (beacon?: string, query?: string) => {
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams();
      if (beacon) params.set('beacon', beacon);
      if (query) params.set('query', query);
      if (!beacon && !query) {
        params.set('query', '*');
      }

      const res = await fetch(`/api/architecture?${params}`);
      if (!res.ok) throw new Error(`API error: ${res.status}`);
      const data: GraphResponse = await res.json();
      setGraphData(data);

      if (data.nodes.length === 0) {
        setNodes([]);
        setEdges([]);
        setLoading(false);
        return;
      }

      // Convert to ReactFlow nodes — spread ArchNodeData into Record<string, unknown>
      const rfNodes: AnyNode[] = data.nodes.map(n => ({
        id: n.id,
        type: 'codeNode' as const,
        position: { x: 0, y: 0 },
        data: { ...n } as Record<string, unknown>,
      }));

      const rfEdges: Edge[] = data.edges.map(e => ({
        id: e.id,
        source: e.source,
        target: e.target,
        type: 'smoothstep',
        animated: e.type === 'calls',
        style: {
          stroke: e.ambiguous ? '#4b5563' : '#6b7280',
          strokeDasharray: e.ambiguous ? '5 5' : undefined,
          strokeWidth: e.ambiguous ? 1 : 1.5,
        },
        label: e.ambiguous ? '?' : undefined,
        labelStyle: { fill: '#9ca3af', fontSize: 10 },
      }));

      // Group nodes by file
      const fileGroups = new Map<string, string[]>();
      for (const n of data.nodes) {
        const group = fileGroups.get(n.file_path) || [];
        group.push(n.id);
        fileGroups.set(n.file_path, group);
      }

      // Compute layout
      try {
        const layouted = await computeLayout(rfNodes, rfEdges, fileGroups);
        setNodes(layouted.nodes);
        setEdges(layouted.edges);
      } catch (layoutErr) {
        console.warn('ELK layout failed, using grid:', layoutErr);
        const gridNodes = rfNodes.map((n, i) => ({
          ...n,
          position: { x: (i % 5) * 280, y: Math.floor(i / 5) * 140 },
        }));
        setNodes(gridNodes);
        setEdges(rfEdges);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [setNodes, setEdges]);

  useEffect(() => {
    if (selectedBeacon) {
      fetchGraph(selectedBeacon);
    }
  }, [selectedBeacon, fetchGraph]);

  const handleSearch = useCallback(() => {
    if (searchQuery.trim()) {
      fetchGraph(undefined, searchQuery.trim());
    }
  }, [searchQuery, fetchGraph]);

  const handleScreenshot = useCallback(() => {
    alert('Use Cmd+Shift+4 (macOS) to capture the graph area, then paste to Claude.');
  }, []);

  const minimapNodeColor = useCallback((node: AnyNode) => {
    if (node.type === 'group') return 'rgba(50,50,50,0.5)';
    const d = node.data as ArchNodeData | undefined;
    return getBeaconColor(d?.beacon);
  }, []);

  const stats = useMemo(() => {
    if (!graphData) return null;
    return {
      nodes: graphData.node_count,
      edges: graphData.edge_count,
      files: graphData.files.length,
    };
  }, [graphData]);

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-neutral-800">
        <div className="flex items-center gap-1.5">
          <Filter className="w-3.5 h-3.5 text-neutral-500" />
          <select
            value={selectedBeacon}
            onChange={e => setSelectedBeacon(e.target.value)}
            className="bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-300 focus:outline-none focus:border-neutral-500"
          >
            <option value="">Select beacon...</option>
            {beacons.map(b => (
              <option key={b} value={b}>{b}</option>
            ))}
          </select>
        </div>

        <div className="flex items-center gap-1 flex-1 max-w-sm">
          <div className="relative flex-1">
            <Search className="w-3.5 h-3.5 text-neutral-500 absolute left-2 top-1/2 -translate-y-1/2" />
            <input
              type="text"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleSearch()}
              placeholder="Search functions, structs..."
              className="w-full bg-neutral-900 border border-neutral-700 rounded pl-7 pr-2 py-1 text-xs text-neutral-300 placeholder:text-neutral-600 focus:outline-none focus:border-neutral-500"
            />
          </div>
          <button
            onClick={handleSearch}
            disabled={loading}
            className="p-1 text-neutral-500 hover:text-neutral-300 transition-colors disabled:opacity-50"
          >
            <Search className="w-3.5 h-3.5" />
          </button>
        </div>

        <div className="flex items-center gap-1 ml-auto">
          {stats && (
            <span className="text-[10px] text-neutral-600 mr-2">
              {stats.nodes} nodes / {stats.edges} edges / {stats.files} files
            </span>
          )}
          <button
            onClick={() => fetchGraph(selectedBeacon || undefined, searchQuery || undefined)}
            disabled={loading}
            className="p-1.5 text-neutral-500 hover:text-neutral-300 transition-colors disabled:opacity-50"
            title="Refresh"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button
            onClick={handleScreenshot}
            className="p-1.5 text-neutral-500 hover:text-neutral-300 transition-colors"
            title="Screenshot"
          >
            <Camera className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Graph */}
      <div className="flex-1 min-h-0">
        {error ? (
          <div className="flex items-center justify-center h-full text-red-400 text-sm">
            {error}
          </div>
        ) : nodes.length === 0 && !loading ? (
          <div className="flex flex-col items-center justify-center h-full text-neutral-500 gap-3">
            <p className="text-sm">Select a beacon or search to visualize architecture</p>
            <div className="flex flex-wrap gap-1.5 max-w-lg justify-center">
              {Object.entries(BEACON_COLORS).map(([name, color]) => (
                <button
                  key={name}
                  onClick={() => setSelectedBeacon(name)}
                  className="px-2 py-1 text-[10px] rounded border transition-colors hover:opacity-80"
                  style={{
                    borderColor: `${color}40`,
                    color,
                    backgroundColor: `${color}10`,
                  }}
                >
                  {name}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            nodeTypes={nodeTypes}
            fitView
            minZoom={0.1}
            maxZoom={2}
            defaultEdgeOptions={{ type: 'smoothstep' }}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} color="#1a1a1a" />
            <Controls className="!bg-neutral-900 !border-neutral-700 !shadow-lg" showInteractive={false} />
            <MiniMap
              nodeColor={minimapNodeColor}
              maskColor="rgba(0, 0, 0, 0.7)"
              className="!bg-neutral-900 !border-neutral-700"
            />
            {loading && (
              <Panel position="top-center">
                <div className="flex items-center gap-2 bg-neutral-900 border border-neutral-700 rounded-lg px-3 py-1.5 text-xs text-neutral-400">
                  <RefreshCw className="w-3 h-3 animate-spin" />
                  Loading graph...
                </div>
              </Panel>
            )}
          </ReactFlow>
        )}
      </div>
    </div>
  );
}
