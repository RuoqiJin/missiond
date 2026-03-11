'use client';

import { memo, useState } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { getBeaconColor, type ArchNodeData } from './types';
import { useArchStore } from './useArchStore';

const NODE_TYPE_SHAPES: Record<string, string> = {
  function: 'rounded-lg',
  struct: 'rounded-none',
  enum: 'rounded-none',
  trait: 'rotate-45-container',
  impl: 'rounded-lg border-dashed',
  mod: 'rounded-lg',
  const: 'rounded-sm',
  type: 'rounded-sm',
};

const NODE_TYPE_ICONS: Record<string, string> = {
  function: 'fn',
  struct: 'S',
  enum: 'E',
  trait: 'T',
  impl: 'I',
  mod: 'M',
  const: 'C',
  type: 'ty',
};

function CodeNode({ data }: NodeProps) {
  const [showStub, setShowStub] = useState(false);
  const d = data as unknown as ArchNodeData;
  const changes = useArchStore(s => s.changes);
  const color = getBeaconColor(d.beacon);
  const shape = NODE_TYPE_SHAPES[d.node_type] || 'rounded-lg';
  const icon = NODE_TYPE_ICONS[d.node_type] || '?';
  const maxLines = 200; // For heat bar scaling
  const heatPct = Math.min(100, Math.round((d.lines / maxLines) * 100));

  // Check if this node has been moved
  const moveChange = changes.findLast(c => c.type === 'move' && c.nodeId === d.id);
  const isMoved = !!moveChange;

  return (
    <div
      className={`relative group ${shape} ${d.is_exported ? 'border-2' : 'border border-dashed'}`}
      style={{
        borderColor: isMoved ? '#3b82f6' : color,
        backgroundColor: isMoved ? 'rgba(59, 130, 246, 0.1)' : `${color}10`,
        minWidth: 180,
        maxWidth: 280,
      }}
      onMouseEnter={() => setShowStub(true)}
      onMouseLeave={() => setShowStub(false)}
    >
      <Handle type="target" position={Position.Top} className="!w-2 !h-2 !bg-neutral-500" />

      {/* Moved indicator */}
      {isMoved && (
        <div className="absolute -top-2 left-1/2 -translate-x-1/2 px-1.5 py-0.5 bg-blue-500 text-[8px] text-white font-medium rounded-full whitespace-nowrap">
          MOVED
        </div>
      )}

      {/* Header */}
      <div className="flex items-center gap-1.5 px-2 py-1.5">
        <span
          className="text-[10px] font-bold px-1 rounded"
          style={{ backgroundColor: `${color}30`, color }}
        >
          {icon}
        </span>
        <span className="text-xs font-medium text-white truncate">{d.name}</span>
        {d.beacon && (
          <span
            className="text-[9px] px-1 rounded ml-auto"
            style={{ backgroundColor: `${color}20`, color }}
          >
            {d.beacon}
          </span>
        )}
      </div>

      {/* Signature */}
      <div className="px-2 pb-1.5">
        <p className="text-[10px] text-neutral-400 font-mono truncate" title={d.signature}>
          {d.signature}
        </p>
      </div>

      {/* Heat bar (lines of code) */}
      {d.lines > 0 && (
        <div className="px-2 pb-1.5">
          <div className="h-1 bg-neutral-800 rounded-full overflow-hidden">
            <div
              className="h-full rounded-full transition-all"
              style={{
                width: `${heatPct}%`,
                backgroundColor: heatPct > 70 ? '#ef4444' : heatPct > 40 ? '#f59e0b' : '#22c55e',
              }}
            />
          </div>
          <span className="text-[9px] text-neutral-600">{d.lines} lines</span>
        </div>
      )}

      {/* Stub tooltip */}
      {showStub && d.stub && (
        <div className="absolute z-50 top-full left-0 mt-1 p-2 bg-neutral-900 border border-neutral-700 rounded-lg shadow-xl max-w-md max-h-48 overflow-auto">
          <pre className="text-[10px] text-neutral-300 font-mono whitespace-pre-wrap">{d.stub}</pre>
          {d.annotation && (
            <p className="text-[10px] text-amber-400 mt-1 border-t border-neutral-700 pt-1">
              {d.annotation}
            </p>
          )}
        </div>
      )}

      <Handle type="source" position={Position.Bottom} className="!w-2 !h-2 !bg-neutral-500" />
    </div>
  );
}

export default memo(CodeNode);
