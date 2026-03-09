import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function JsonTreeViewer({ data }: { data: any }) {
  return (
    <div className="bg-[#0d1117] text-[#c9d1d9] font-mono text-[11px] p-3 rounded-lg overflow-auto border border-neutral-800">
      <JsonNode value={data} isRoot />
    </div>
  );
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function JsonNode({ value, name, isRoot = false }: { value: any; name?: string; isRoot?: boolean }) {
  const [expanded, setExpanded] = useState(true);
  const isObject = value !== null && typeof value === 'object';
  const isArray = Array.isArray(value);

  if (!isObject) {
    let color = 'text-[#a5d6ff]';
    if (typeof value === 'number') color = 'text-[#79c0ff]';
    if (typeof value === 'boolean') color = 'text-[#ff7b72]';
    if (value === null) color = 'text-[#8b949e]';

    return (
      <div className="flex leading-relaxed" style={{ marginLeft: isRoot ? 0 : 16 }}>
        {name != null && <span className="text-[#7ee787] mr-1">&quot;{name}&quot;:</span>}
        <span className={color}>
          {typeof value === 'string' ? `"${value}"` : String(value)}
        </span>
      </div>
    );
  }

  const keys = Object.keys(value);
  const isEmpty = keys.length === 0;
  const bracket = isArray ? ['[', ']'] : ['{', '}'];

  return (
    <div style={{ marginLeft: isRoot ? 0 : 16 }} className="leading-relaxed">
      <div
        className={cn('flex items-center w-fit pr-2 rounded', !isEmpty && 'cursor-pointer hover:bg-white/5')}
        onClick={() => !isEmpty && setExpanded(!expanded)}
      >
        {!isEmpty ? (
          expanded ? <ChevronDown className="w-3 h-3 text-neutral-500 mr-1 shrink-0" /> : <ChevronRight className="w-3 h-3 text-neutral-500 mr-1 shrink-0" />
        ) : <span className="w-4 shrink-0" />}
        {name != null && <span className="text-[#7ee787] mr-1">&quot;{name}&quot;:</span>}
        <span className="text-neutral-400">{bracket[0]}</span>
        {!expanded && !isEmpty && <span className="text-neutral-500 px-1">…{keys.length}</span>}
        {(!expanded || isEmpty) && <span className="text-neutral-400">{bracket[1]}</span>}
      </div>
      {expanded && !isEmpty && (
        <>
          {keys.map((key) => (
            <JsonNode key={key} name={isArray ? undefined : key} value={value[key as keyof typeof value]} />
          ))}
          <div style={{ marginLeft: 16 }} className="text-neutral-400">{bracket[1]}</div>
        </>
      )}
    </div>
  );
}
