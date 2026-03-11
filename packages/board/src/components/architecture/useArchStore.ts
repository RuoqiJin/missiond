import { create } from 'zustand';
import type { ArchNodeData, ArchEdge } from './types';

export interface ArchChange {
  type: 'move' | 'add_edge' | 'remove_edge' | 'new_file';
  // move
  nodeId?: string;
  symbol?: string;
  signature?: string;
  fromFile?: string;
  toFile?: string;
  // edge
  sourceId?: string;
  sourceSymbol?: string;
  targetId?: string;
  targetSymbol?: string;
  edgeType?: string;
  // new_file
  filePath?: string;
}

export interface RefactorDiff {
  description: string;
  moves: {
    id: string;
    symbol: string;
    signature: string;
    from_file: string;
    to_file: string;
  }[];
  add_edges: {
    from_id: string;
    from_symbol: string;
    to_id: string;
    to_symbol: string;
    type: string;
  }[];
  remove_edges: {
    from_id: string;
    from_symbol: string;
    to_id: string;
    to_symbol: string;
  }[];
  new_files: string[];
}

interface ArchStore {
  // Original state snapshot
  originalNodes: ArchNodeData[];
  originalEdges: ArchEdge[];

  // Change log (event sourcing)
  changes: ArchChange[];
  undoStack: ArchChange[][];
  redoStack: ArchChange[][];

  // Edit mode
  editMode: boolean;
  showDeletedEdges: boolean;

  // Actions
  setOriginal: (nodes: ArchNodeData[], edges: ArchEdge[]) => void;
  setEditMode: (on: boolean) => void;
  setShowDeletedEdges: (show: boolean) => void;

  recordMove: (nodeId: string, symbol: string, signature: string, fromFile: string, toFile: string) => void;
  recordAddEdge: (sourceId: string, sourceSymbol: string, targetId: string, targetSymbol: string, edgeType: string) => void;
  recordRemoveEdge: (sourceId: string, sourceSymbol: string, targetId: string, targetSymbol: string) => void;
  recordNewFile: (filePath: string) => void;

  undo: () => void;
  redo: () => void;
  reset: () => void;

  // Computed
  hasPendingChanges: () => boolean;
  generateDiff: () => RefactorDiff;
}

export const useArchStore = create<ArchStore>((set, get) => ({
  originalNodes: [],
  originalEdges: [],
  changes: [],
  undoStack: [],
  redoStack: [],
  editMode: false,
  showDeletedEdges: true,

  setOriginal: (nodes, edges) => set({
    originalNodes: nodes,
    originalEdges: edges,
    changes: [],
    undoStack: [],
    redoStack: [],
  }),

  setEditMode: (on) => set({ editMode: on }),
  setShowDeletedEdges: (show) => set({ showDeletedEdges: show }),

  recordMove: (nodeId, symbol, signature, fromFile, toFile) => {
    const { changes, undoStack } = get();
    set({
      undoStack: [...undoStack, changes],
      redoStack: [],
      changes: [...changes, { type: 'move', nodeId, symbol, signature, fromFile, toFile }],
    });
  },

  recordAddEdge: (sourceId, sourceSymbol, targetId, targetSymbol, edgeType) => {
    const { changes, undoStack } = get();
    set({
      undoStack: [...undoStack, changes],
      redoStack: [],
      changes: [...changes, { type: 'add_edge', sourceId, sourceSymbol, targetId, targetSymbol, edgeType }],
    });
  },

  recordRemoveEdge: (sourceId, sourceSymbol, targetId, targetSymbol) => {
    const { changes, undoStack } = get();
    set({
      undoStack: [...undoStack, changes],
      redoStack: [],
      changes: [...changes, { type: 'remove_edge', sourceId, sourceSymbol, targetId, targetSymbol }],
    });
  },

  recordNewFile: (filePath) => {
    const { changes, undoStack } = get();
    set({
      undoStack: [...undoStack, changes],
      redoStack: [],
      changes: [...changes, { type: 'new_file', filePath }],
    });
  },

  undo: () => {
    const { undoStack, changes, redoStack } = get();
    if (undoStack.length === 0) return;
    const prev = undoStack[undoStack.length - 1];
    set({
      changes: prev,
      undoStack: undoStack.slice(0, -1),
      redoStack: [...redoStack, changes],
    });
  },

  redo: () => {
    const { redoStack, changes, undoStack } = get();
    if (redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    set({
      changes: next,
      redoStack: redoStack.slice(0, -1),
      undoStack: [...undoStack, changes],
    });
  },

  reset: () => set({ changes: [], undoStack: [], redoStack: [] }),

  hasPendingChanges: () => get().changes.length > 0,

  generateDiff: () => {
    const { changes, originalNodes } = get();
    const nodeMap = new Map(originalNodes.map(n => [n.id, n]));

    // Collapse moves: if same node moved multiple times, only keep last
    const lastMoves = new Map<string, ArchChange>();
    const addEdges: ArchChange[] = [];
    const removeEdges: ArchChange[] = [];
    const newFiles: string[] = [];

    for (const c of changes) {
      switch (c.type) {
        case 'move':
          lastMoves.set(c.nodeId!, c);
          break;
        case 'add_edge':
          // Check if this edge was previously removed — cancel both out
          const rmIdx = removeEdges.findIndex(r =>
            r.sourceId === c.sourceId && r.targetId === c.targetId
          );
          if (rmIdx >= 0) {
            removeEdges.splice(rmIdx, 1);
          } else {
            addEdges.push(c);
          }
          break;
        case 'remove_edge':
          // Check if this edge was previously added — cancel both out
          const addIdx = addEdges.findIndex(a =>
            a.sourceId === c.sourceId && a.targetId === c.targetId
          );
          if (addIdx >= 0) {
            addEdges.splice(addIdx, 1);
          } else {
            removeEdges.push(c);
          }
          break;
        case 'new_file':
          if (c.filePath && !newFiles.includes(c.filePath)) {
            newFiles.push(c.filePath);
          }
          break;
      }
    }

    // Filter out moves that ended up back at original position
    const effectiveMoves = Array.from(lastMoves.values()).filter(m => {
      const orig = nodeMap.get(m.nodeId!);
      return orig && orig.file_path !== m.toFile;
    });

    return {
      description: '用户通过架构可视化工具生成的重构意图',
      moves: effectiveMoves.map(m => ({
        id: m.nodeId!,
        symbol: m.symbol!,
        signature: m.signature!,
        from_file: m.fromFile!,
        to_file: m.toFile!,
      })),
      add_edges: addEdges.map(e => ({
        from_id: e.sourceId!,
        from_symbol: e.sourceSymbol!,
        to_id: e.targetId!,
        to_symbol: e.targetSymbol!,
        type: e.edgeType || 'calls',
      })),
      remove_edges: removeEdges.map(e => ({
        from_id: e.sourceId!,
        from_symbol: e.sourceSymbol!,
        to_id: e.targetId!,
        to_symbol: e.targetSymbol!,
      })),
      new_files: newFiles,
    };
  },
}));
