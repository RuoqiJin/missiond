'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  useReactFlow,
  ReactFlowProvider,
  type Node,
  type Edge,
  type Connection,
  BackgroundVariant,
  Panel,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import ELK, { type ElkNode } from 'elkjs/lib/elk.bundled.js';
import { Search, RefreshCw, Camera, Filter, Pencil, Eye, FilePlus } from 'lucide-react';
import CodeNode from './CodeNode';
import DiffPanel from './DiffPanel';
import { useArchStore } from './useArchStore';
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

function ArchitectureViewInner() {
  const [nodes, setNodes, onNodesChange] = useNodesState<AnyNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [beacons, setBeacons] = useState<string[]>([]);
  const [selectedBeacon, setSelectedBeacon] = useState<string>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [graphData, setGraphData] = useState<GraphResponse | null>(null);
  const [newFileInput, setNewFileInput] = useState('');
  const [showNewFileInput, setShowNewFileInput] = useState(false);

  const { getIntersectingNodes } = useReactFlow();
  const { editMode, setEditMode, setOriginal, recordMove, recordAddEdge, recordRemoveEdge, recordNewFile, hasPendingChanges, undo, redo } = useArchStore();

  // Store original graph data ref for node lookups
  const originalNodesRef = useRef<ArchNodeData[]>([]);

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
      originalNodesRef.current = data.nodes;

      // Save original state to store
      setOriginal(data.nodes, data.edges);

      if (data.nodes.length === 0) {
        setNodes([]);
        setEdges([]);
        setLoading(false);
        return;
      }

      // Convert to ReactFlow nodes
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
  }, [setNodes, setEdges, setOriginal]);

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

  // P2: Cross-container drag detection
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const handleNodeDragStop = useCallback((_: any, node: AnyNode) => {
    if (!editMode) return;
    if (node.type === 'group') return; // Don't process group node drags

    const d = node.data as Record<string, unknown>;
    const currentFile = d.file_path as string;
    const currentParent = node.parentId;

    // Find intersecting group nodes
    const intersecting = getIntersectingNodes(node).filter(n => n.type === 'group');
    if (intersecting.length === 0) return;

    // Pick the most overlapping group
    const targetGroup = intersecting[0];
    const targetGroupId = targetGroup.id;

    // If dropped on same group, no move
    if (targetGroupId === currentParent) return;

    // Extract target file path from group id
    const targetFile = targetGroupId.replace('group-', '');

    // Record the move
    const nodeData = originalNodesRef.current.find(n => n.id === node.id);
    recordMove(
      node.id,
      (d.name as string) || nodeData?.name || '',
      (d.signature as string) || nodeData?.signature || '',
      currentFile,
      targetFile,
    );

    // Update the node's parentId and file_path visually
    setNodes(nds => nds.map(n => {
      if (n.id === node.id) {
        return {
          ...n,
          parentId: targetGroupId,
          extent: 'parent' as const,
          data: { ...n.data, file_path: targetFile },
          // Convert position: absolute → relative to new parent
          position: {
            x: node.position.x + (node.parentId ? getGroupPosition(nds, node.parentId).x : 0) - getGroupPosition(nds, targetGroupId).x,
            y: node.position.y + (node.parentId ? getGroupPosition(nds, node.parentId).y : 0) - getGroupPosition(nds, targetGroupId).y,
          },
        };
      }
      return n;
    }));
  }, [editMode, getIntersectingNodes, recordMove, setNodes]);

  // P2: Edge creation via drag connection
  const handleConnect = useCallback((connection: Connection) => {
    if (!editMode) return;

    const sourceNode = originalNodesRef.current.find(n => n.id === connection.source);
    const targetNode = originalNodesRef.current.find(n => n.id === connection.target);

    recordAddEdge(
      connection.source,
      sourceNode?.name || connection.source,
      connection.target,
      targetNode?.name || connection.target,
      'calls',
    );

    // Add visual edge
    const newEdge: Edge = {
      id: `new-${connection.source}-${connection.target}`,
      source: connection.source,
      target: connection.target,
      type: 'smoothstep',
      animated: true,
      style: { stroke: '#22c55e', strokeWidth: 2 }, // Green for new edges
    };
    setEdges(eds => [...eds, newEdge]);
  }, [editMode, recordAddEdge, setEdges]);

  // P2: Edge deletion
  const handleEdgesDelete = useCallback((deletedEdges: Edge[]) => {
    if (!editMode) return;

    for (const edge of deletedEdges) {
      const sourceNode = originalNodesRef.current.find(n => n.id === edge.source);
      const targetNode = originalNodesRef.current.find(n => n.id === edge.target);

      recordRemoveEdge(
        edge.source,
        sourceNode?.name || edge.source,
        edge.target,
        targetNode?.name || edge.target,
      );
    }
  }, [editMode, recordRemoveEdge]);

  // P2: New file container
  const handleAddNewFile = useCallback(() => {
    if (!newFileInput.trim()) return;
    const filePath = newFileInput.trim();

    recordNewFile(filePath);

    // Add empty group node visually
    const groupId = `group-${filePath}`;
    const existingGroups = nodes.filter(n => n.type === 'group');
    const maxX = existingGroups.reduce((max, g) => Math.max(max, g.position.x + ((g.style?.width as number) || 260)), 0);

    setNodes(nds => [...nds, {
      id: groupId,
      type: 'group',
      position: { x: maxX + 40, y: 0 },
      data: { label: shortPath(filePath) },
      style: {
        width: 260,
        height: 200,
        backgroundColor: 'rgba(30, 30, 30, 0.5)',
        borderColor: 'rgba(245, 158, 11, 0.5)', // amber for new files
        borderWidth: 2,
        borderRadius: 8,
        padding: 10,
      },
    }]);

    setNewFileInput('');
    setShowNewFileInput(false);
  }, [newFileInput, nodes, recordNewFile, setNodes]);

  // P2: Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === 'z' && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (e.metaKey && e.shiftKey && e.key === 'z') {
        e.preventDefault();
        redo();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [undo, redo]);

  // P2: beforeunload protection
  useEffect(() => {
    const handler = (e: BeforeUnloadEvent) => {
      if (hasPendingChanges()) {
        e.preventDefault();
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [hasPendingChanges]);

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

          {/* Edit mode toggle */}
          <button
            onClick={() => setEditMode(!editMode)}
            className={`flex items-center gap-1 px-2 py-1 text-[11px] rounded transition-colors ${
              editMode
                ? 'bg-orange-500/20 text-orange-400 border border-orange-500/30'
                : 'text-neutral-500 hover:text-neutral-300 border border-transparent'
            }`}
            title={editMode ? 'Switch to view mode' : 'Switch to edit mode'}
          >
            {editMode ? <Pencil className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
            {editMode ? 'Editing' : 'View'}
          </button>

          {/* New file button (edit mode only) */}
          {editMode && (
            <button
              onClick={() => setShowNewFileInput(!showNewFileInput)}
              className="p-1.5 text-neutral-500 hover:text-amber-400 transition-colors"
              title="Add new file container"
            >
              <FilePlus className="w-3.5 h-3.5" />
            </button>
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

      {/* New file input bar */}
      {showNewFileInput && (
        <div className="flex items-center gap-2 px-4 py-1.5 border-b border-neutral-800 bg-neutral-900/50">
          <span className="text-[10px] text-amber-400">New file:</span>
          <input
            type="text"
            value={newFileInput}
            onChange={e => setNewFileInput(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && handleAddNewFile()}
            placeholder="crates/missiond-daemon/src/new_module.rs"
            className="flex-1 max-w-md bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-300 placeholder:text-neutral-600 focus:outline-none focus:border-amber-500/50"
            autoFocus
          />
          <button onClick={handleAddNewFile} className="px-2 py-1 text-[11px] bg-amber-500/20 text-amber-400 rounded hover:bg-amber-500/30 transition-colors">
            Add
          </button>
          <button onClick={() => setShowNewFileInput(false)} className="text-neutral-500 hover:text-neutral-300 text-xs">
            Cancel
          </button>
        </div>
      )}

      {/* Graph + Diff Panel */}
      <div className="flex-1 min-h-0 relative">
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
            onNodeDragStop={handleNodeDragStop}
            onConnect={editMode ? handleConnect : undefined}
            onEdgesDelete={editMode ? handleEdgesDelete : undefined}
            nodeTypes={nodeTypes}
            fitView
            minZoom={0.1}
            maxZoom={2}
            defaultEdgeOptions={{ type: 'smoothstep' }}
            proOptions={{ hideAttribution: true }}
            deleteKeyCode={editMode ? 'Delete' : null}
            connectOnClick={editMode}
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
            {editMode && (
              <Panel position="top-left">
                <div className="bg-orange-500/10 border border-orange-500/20 rounded-lg px-3 py-1.5 text-[11px] text-orange-400">
                  Edit mode — Drag nodes between files, connect/delete edges, add files
                </div>
              </Panel>
            )}
          </ReactFlow>
        )}

        {/* Diff Panel */}
        <DiffPanel />
      </div>
    </div>
  );
}

/** Get a group node's absolute position from the nodes array */
function getGroupPosition(nodes: AnyNode[], groupId: string): { x: number; y: number } {
  const group = nodes.find(n => n.id === groupId);
  return group ? group.position : { x: 0, y: 0 };
}

export default function ArchitectureView() {
  return (
    <ReactFlowProvider>
      <ArchitectureViewInner />
    </ReactFlowProvider>
  );
}
