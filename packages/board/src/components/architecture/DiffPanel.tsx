'use client';

import { useState } from 'react';
import { X, Copy, Check, ArrowRight, Plus, Minus, FilePlus, Undo2, Redo2 } from 'lucide-react';
import { useArchStore } from './useArchStore';

export default function DiffPanel() {
  const { changes, hasPendingChanges, generateDiff, reset, undo, redo, undoStack, redoStack } = useArchStore();
  const [copied, setCopied] = useState(false);

  if (!hasPendingChanges()) return null;

  const diff = generateDiff();
  const totalChanges = diff.moves.length + diff.add_edges.length + diff.remove_edges.length + diff.new_files.length;

  const handleCopy = () => {
    const json = JSON.stringify(diff, null, 2);
    navigator.clipboard.writeText(json).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  const handleCreateTask = async () => {
    try {
      const res = await fetch('/api/tasks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          title: `Refactor: ${diff.moves.length} moves, ${diff.add_edges.length + diff.remove_edges.length} edge changes`,
          description: `## Architecture Refactoring\n\nGenerated from visual architecture editor.\n\n\`\`\`json\n${JSON.stringify(diff, null, 2)}\n\`\`\``,
          category: 'refactor',
          priority: 'medium',
        }),
      });
      if (res.ok) {
        alert('Task created successfully');
      }
    } catch {
      alert('Failed to create task');
    }
  };

  return (
    <div className="absolute right-0 top-0 bottom-0 w-80 bg-neutral-900 border-l border-neutral-700 z-50 flex flex-col shadow-2xl">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800">
        <span className="text-xs font-medium text-neutral-300">
          Refactoring Diff ({totalChanges} {totalChanges === 1 ? 'change' : 'changes'})
        </span>
        <div className="flex items-center gap-1">
          <button
            onClick={undo}
            disabled={undoStack.length === 0}
            className="p-1 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 transition-colors"
            title="Undo (⌘Z)"
          >
            <Undo2 className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={redo}
            disabled={redoStack.length === 0}
            className="p-1 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 transition-colors"
            title="Redo (⇧⌘Z)"
          >
            <Redo2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Change list */}
      <div className="flex-1 overflow-auto p-2 space-y-1.5">
        {diff.moves.map((m, i) => (
          <div key={`move-${i}`} className="bg-blue-500/10 border border-blue-500/20 rounded px-2 py-1.5">
            <div className="flex items-center gap-1.5 text-blue-400">
              <ArrowRight className="w-3 h-3 shrink-0" />
              <span className="text-[11px] font-medium">MOVE</span>
              <span className="text-[11px] text-blue-300 truncate">{m.symbol}</span>
            </div>
            <div className="text-[10px] text-neutral-500 mt-0.5 pl-4">
              {shortFilePath(m.from_file)} → {shortFilePath(m.to_file)}
            </div>
          </div>
        ))}

        {diff.add_edges.map((e, i) => (
          <div key={`add-${i}`} className="bg-emerald-500/10 border border-emerald-500/20 rounded px-2 py-1.5">
            <div className="flex items-center gap-1.5 text-emerald-400">
              <Plus className="w-3 h-3 shrink-0" />
              <span className="text-[11px] font-medium">EDGE</span>
              <span className="text-[10px] text-emerald-300 truncate">
                {e.from_symbol} → {e.to_symbol}
              </span>
            </div>
            <div className="text-[10px] text-neutral-500 mt-0.5 pl-4">
              type: {e.type}
            </div>
          </div>
        ))}

        {diff.remove_edges.map((e, i) => (
          <div key={`rm-${i}`} className="bg-red-500/10 border border-red-500/20 rounded px-2 py-1.5">
            <div className="flex items-center gap-1.5 text-red-400">
              <Minus className="w-3 h-3 shrink-0" />
              <span className="text-[11px] font-medium">EDGE</span>
              <span className="text-[10px] text-red-300 truncate">
                {e.from_symbol} → {e.to_symbol}
              </span>
            </div>
          </div>
        ))}

        {diff.new_files.map((f, i) => (
          <div key={`file-${i}`} className="bg-amber-500/10 border border-amber-500/20 rounded px-2 py-1.5">
            <div className="flex items-center gap-1.5 text-amber-400">
              <FilePlus className="w-3 h-3 shrink-0" />
              <span className="text-[11px] font-medium">NEW FILE</span>
              <span className="text-[10px] text-amber-300 truncate">{f}</span>
            </div>
          </div>
        ))}

        {totalChanges === 0 && changes.length > 0 && (
          <div className="text-[11px] text-neutral-500 text-center py-4">
            All changes cancel out (e.g., moved back to original position)
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2 px-3 py-2 border-t border-neutral-800">
        <button
          onClick={handleCopy}
          className="flex items-center gap-1.5 px-2.5 py-1.5 text-[11px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 rounded transition-colors"
        >
          {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
          {copied ? 'Copied' : 'Copy JSON'}
        </button>
        <button
          onClick={handleCreateTask}
          className="flex items-center gap-1.5 px-2.5 py-1.5 text-[11px] bg-orange-500/20 hover:bg-orange-500/30 text-orange-400 rounded transition-colors"
        >
          Create Task
        </button>
        <button
          onClick={reset}
          className="flex items-center gap-1.5 px-2.5 py-1.5 text-[11px] text-neutral-500 hover:text-red-400 ml-auto transition-colors"
          title="Discard all changes"
        >
          <X className="w-3 h-3" />
          Reset
        </button>
      </div>
    </div>
  );
}

function shortFilePath(path: string): string {
  const parts = path.split('/');
  const srcIdx = parts.indexOf('src');
  if (srcIdx > 0) {
    const crate = parts[srcIdx - 1].replace('missiond-', '');
    return `${crate}/${parts.slice(srcIdx + 1).join('/')}`;
  }
  return parts.slice(-2).join('/');
}
