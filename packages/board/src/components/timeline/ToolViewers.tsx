import { useState, useEffect, useMemo } from 'react';
import {
  Search, ChevronDown, ChevronRight, Wrench,
  FileCode, File, Eye, Terminal, BookOpen, Loader2,
} from 'lucide-react';
import { diffLines } from 'diff';
import { cn } from '@/lib/utils';
import { MarkdownContent } from './MarkdownContent';

// ── Tool Call Viewers (pluggable registry) ──────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ToolViewerRegistry: Record<string, React.FC<{ input: any; block?: any }>> = {
  Edit: EditDiffViewer,
  Write: WriteSummaryViewer,
  Skill: ToolResultViewer,
  Read: ToolResultViewer,
};

/** Render content blocks from raw_content (text + tool_use interleaved) */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function ContentBlocksRenderer({ blocks }: { blocks: any[] }) {
  const rendered = useMemo(() => {
    const items: { type: 'text'; text: string }[] | { type: 'tool'; block: Record<string, unknown> }[] = [];
    let textBuf = '';

    for (const block of blocks) {
      const btype = block?.type;
      if (btype === 'text' && block.text) {
        textBuf += (textBuf ? '\n' : '') + block.text;
      } else if (btype === 'tool_use') {
        if (textBuf) { items.push({ type: 'text', text: textBuf } as never); textBuf = ''; }
        items.push({ type: 'tool', block } as never);
      }
      // skip thinking, tool_result, etc.
    }
    if (textBuf) items.push({ type: 'text', text: textBuf } as never);
    return items;
  }, [blocks]);

  return (
    <div className="space-y-2">
      {rendered.map((item, i) => {
        if (item.type === 'text') {
          const t = item as { type: 'text'; text: string };
          return (
            <div key={i} className="p-3 rounded-lg text-sm leading-relaxed max-h-[600px] overflow-auto bg-teal-500/10 border border-teal-500/20">
              <MarkdownContent content={t.text} />
            </div>
          );
        }
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const tool = item as { type: 'tool'; block: any };
        return <ToolCallCard key={i} block={tool.block} />;
      })}
    </div>
  );
}

/** Generic tool call card with collapsible body and pluggable viewer */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ToolCallCard({ block }: { block: any }) {
  const [expanded, setExpanded] = useState(true);
  const name: string = block.name || 'unknown';
  const input = block.input || {};
  const Viewer = ToolViewerRegistry[name];

  const filePath = input.file_path || input.path || input.command;
  const subtitle = typeof filePath === 'string'
    ? filePath.replace(/^.*\/Projects\//, '~/Projects/')
    : null;

  const iconMap: Record<string, React.ReactNode> = {
    Edit: <FileCode className="w-3 h-3" />,
    Write: <File className="w-3 h-3" />,
    Read: <Eye className="w-3 h-3" />,
    Bash: <Terminal className="w-3 h-3" />,
    Grep: <Search className="w-3 h-3" />,
    Skill: <BookOpen className="w-3 h-3" />,
  };

  return (
    <div className="border border-neutral-800 rounded-md overflow-hidden">
      <div
        className="bg-neutral-900/80 px-3 py-1.5 flex items-center gap-2 cursor-pointer hover:bg-neutral-800/60 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? <ChevronDown className="w-3 h-3 text-neutral-500" /> : <ChevronRight className="w-3 h-3 text-neutral-500" />}
        <span className="text-teal-400 font-mono text-xs font-medium">{iconMap[name] || <Wrench className="w-3 h-3" />}</span>
        <span className="text-teal-400 font-mono text-xs">{name}</span>
        {subtitle && <span className="text-neutral-500 text-[10px] font-mono truncate">{subtitle}</span>}
      </div>
      {expanded && (
        <div className="bg-neutral-950">
          {Viewer ? <Viewer input={input} block={block} /> : <FallbackToolViewer name={name} input={input} />}
        </div>
      )}
    </div>
  );
}

/** Edit tool: diff viewer with red/green highlighting */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function EditDiffViewer({ input }: { input: any }) {
  const { file_path, old_string, new_string } = input;

  const changes = useMemo(() => {
    if (!old_string && !new_string) return [];
    return diffLines(old_string || '', new_string || '');
  }, [old_string, new_string]);

  // Compute line numbers
  const lineData = useMemo(() => {
    let oldLine = 1;
    let newLine = 1;
    const result: { prefix: string; oldNum: string; newNum: string; text: string; kind: 'add' | 'del' | 'ctx' }[] = [];

    for (const part of changes) {
      const lines = part.value.replace(/\n$/, '').split('\n');
      for (const line of lines) {
        if (part.added) {
          result.push({ prefix: '+', oldNum: '', newNum: String(newLine++), text: line, kind: 'add' });
        } else if (part.removed) {
          result.push({ prefix: '-', oldNum: String(oldLine++), newNum: '', text: line, kind: 'del' });
        } else {
          result.push({ prefix: ' ', oldNum: String(oldLine++), newNum: String(newLine++), text: line, kind: 'ctx' });
        }
      }
    }
    return result;
  }, [changes]);

  const shortPath = file_path?.replace(/^.*\/Projects\//, '~/Projects/') || 'unknown';

  return (
    <div className="flex flex-col text-[11px] font-mono">
      <div className="px-3 py-1.5 bg-neutral-800/40 border-b border-neutral-800 text-neutral-400 flex items-center gap-2">
        <FileCode className="w-3 h-3 text-neutral-500" />
        <span className="truncate">{shortPath}</span>
      </div>
      <div className="overflow-auto max-h-[500px]">
        {lineData.map((ln, i) => (
          <div
            key={i}
            className={cn(
              'flex leading-relaxed',
              ln.kind === 'add' && 'bg-green-500/10',
              ln.kind === 'del' && 'bg-red-500/10',
            )}
          >
            <span className="select-none text-neutral-600 w-8 text-right pr-1 shrink-0">{ln.oldNum}</span>
            <span className="select-none text-neutral-600 w-8 text-right pr-1 shrink-0 border-r border-neutral-800">{ln.newNum}</span>
            <span className={cn(
              'select-none w-4 text-center shrink-0',
              ln.kind === 'add' && 'text-green-400',
              ln.kind === 'del' && 'text-red-400',
              ln.kind === 'ctx' && 'text-neutral-600',
            )}>{ln.prefix}</span>
            <span className={cn(
              'whitespace-pre-wrap break-all px-1',
              ln.kind === 'add' && 'text-green-300',
              ln.kind === 'del' && 'text-red-300',
              ln.kind === 'ctx' && 'text-neutral-300',
            )}>{ln.text}</span>
          </div>
        ))}
        {lineData.length === 0 && (
          <div className="px-3 py-2 text-neutral-500 italic">No changes</div>
        )}
      </div>
    </div>
  );
}

/** Write tool: show file path and content preview */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function WriteSummaryViewer({ input }: { input: any }) {
  const { file_path, content } = input;
  const shortPath = file_path?.replace(/^.*\/Projects\//, '~/Projects/') || 'unknown';
  const preview = typeof content === 'string' ? content.slice(0, 500) : '';
  return (
    <div className="flex flex-col text-[11px] font-mono">
      <div className="px-3 py-1.5 bg-neutral-800/40 border-b border-neutral-800 text-neutral-400 flex items-center gap-2">
        <File className="w-3 h-3 text-neutral-500" />
        <span className="truncate">{shortPath}</span>
        {content && <span className="text-neutral-600 ml-auto">{content.length} chars</span>}
      </div>
      <pre className="p-3 text-green-300/80 whitespace-pre-wrap break-all max-h-64 overflow-auto">{preview}{content && content.length > 500 ? '\n...' : ''}</pre>
    </div>
  );
}

/** Lazy-load tool result from audit_detail API, shared by Skill/Read viewers */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ToolResultViewer({ input, block }: { input: any; block?: any }) {
  const toolId: string | undefined = block?.id;
  const [detail, setDetail] = useState<{ input?: unknown; output?: unknown } | null>(null);
  const [loading, setLoading] = useState(false);

  // Auto-load tool result on mount (user preference: show full content directly)
  useEffect(() => {
    if (!toolId || detail) return;
    setLoading(true);
    fetch(`/api/system/tool-call?id=${encodeURIComponent(toolId)}`)
      .then(r => r.json())
      .then(data => { if (!data.error) setDetail(data); })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [toolId, detail]);

  // Extract readable text from tool result
  const resultText = useMemo(() => {
    if (!detail?.output) return '';
    const out = detail.output;
    if (typeof out === 'string') return out;
    // Handle array of content blocks [{type:'text', text:'...'}]
    if (Array.isArray(out)) {
      return out
        .filter((b: Record<string, unknown>) => b.type === 'text')
        .map((b: Record<string, unknown>) => b.text)
        .join('\n');
    }
    // Handle object with text/content field
    if (typeof out === 'object' && out !== null) {
      const o = out as Record<string, unknown>;
      if (typeof o.text === 'string') return o.text;
      if (typeof o.content === 'string') return o.content;
      return JSON.stringify(out, null, 2);
    }
    return String(out);
  }, [detail]);

  const toolName = block?.name || 'Tool';
  const isSkill = toolName === 'Skill';
  const isRead = toolName === 'Read';

  // For Skill: derive file path from skill name; for Read: use input.file_path
  const displayPath = isSkill
    ? (input?.skill ? `~/.claude/skills/${input.skill}/SKILL.md` : null)
    : (input?.file_path?.replace(/^.*\/Projects\//, '~/Projects/') || input?.path || null);

  return (
    <div className="p-3 space-y-2">
      {/* Input params */}
      <div className="space-y-1">
        {Object.entries(input || {}).filter(([, v]) => v != null).map(([k, v]) => {
          const val = typeof v === 'string' ? v : JSON.stringify(v);
          // Truncate long values in input display (e.g., Read's file_path is shown above)
          const display = val.length > 200 ? val.slice(0, 200) + '...' : val;
          return (
            <div key={k} className="flex gap-2 text-[11px] font-mono">
              <span className="text-neutral-500 shrink-0">{k}:</span>
              <span className="text-neutral-300 break-all">{display}</span>
            </div>
          );
        })}
      </div>

      {/* Derived path for Skill */}
      {isSkill && displayPath && (
        <div className="flex items-center gap-1.5 text-[11px] font-mono text-amber-400/80">
          <BookOpen className="w-3 h-3" />
          <span>{displayPath}</span>
        </div>
      )}

      {/* Tool result — auto-loaded */}
      {loading && (
        <div className="flex items-center gap-1.5 text-[11px] text-neutral-500">
          <Loader2 className="w-3 h-3 animate-spin" />
          Loading...
        </div>
      )}

      {resultText && (
        <div className="border border-neutral-800 rounded overflow-hidden">
          <div className="bg-neutral-900/60 px-2 py-1 flex items-center gap-1.5 text-[10px] text-neutral-400 border-b border-neutral-800">
            {isRead ? <Eye className="w-3 h-3" /> : <BookOpen className="w-3 h-3" />}
            <span className="font-mono truncate">{displayPath || 'Result'}</span>
            <span className="text-neutral-600 ml-auto">{resultText.length} chars</span>
          </div>
          <pre className="p-2 text-[11px] font-mono text-neutral-300/80 whitespace-pre-wrap break-words max-h-80 overflow-auto leading-relaxed">
            {resultText}
          </pre>
        </div>
      )}

      {!loading && !resultText && detail && (
        <span className="text-[10px] text-neutral-500 italic">No result data</span>
      )}
    </div>
  );
}

/** Fallback: show tool input as compact key-value pairs */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function FallbackToolViewer({ input }: { name: string; input: any }) {
  const entries = Object.entries(input || {}).filter(([, v]) => v != null);
  return (
    <div className="p-3 space-y-1">
      {entries.length === 0 ? (
        <span className="text-neutral-500 text-xs italic">No input parameters</span>
      ) : entries.map(([k, v]) => {
        const val = typeof v === 'string' ? (v.length > 200 ? v.slice(0, 200) + '...' : v) : JSON.stringify(v);
        return (
          <div key={k} className="flex gap-2 text-[11px] font-mono">
            <span className="text-neutral-500 shrink-0">{k}:</span>
            <span className="text-neutral-300 break-all whitespace-pre-wrap">{val}</span>
          </div>
        );
      })}
    </div>
  );
}
