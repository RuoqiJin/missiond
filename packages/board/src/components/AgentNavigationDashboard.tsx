'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Check, Compass, Loader2, Search, Send, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type NavigationEntry = {
  id?: string;
  projectId?: string;
  label?: string;
  primaryFamily?: string;
  coverageState?: string;
  readFirst?: string[];
  writeScope?: string[];
  mustNotTouch?: string[];
  checks?: string[];
  authorityNotes?: string[];
};

type CatalogResponse = {
  ok?: boolean;
  error?: string;
  selectedEntry?: NavigationEntry | null;
  entries?: NavigationEntry[];
  projects?: NavigationEntry[];
  review?: { eventCount?: number; outcomes?: Record<string, number> };
  diagnostic?: { code?: string; message?: string };
};

const DEFAULT_INTENT = '我要修改 autopilot 的 BoardTask 完成判定';

function toErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error || 'Unknown error');
}

async function fetchNavigation(action: 'catalog' | 'guide', project: string, intent: string): Promise<CatalogResponse> {
  const params = new URLSearchParams({ action, project });
  if (intent.trim()) params.set('intent', intent.trim());

  try {
    const res = await fetch(`/api/agent-navigation?${params.toString()}`);
    const data = (await res.json().catch((error: unknown) => ({
      ok: false,
      error: 'INVALID_RESPONSE',
      diagnostic: { code: 'INVALID_RESPONSE', message: toErrorMessage(error) },
    }))) as CatalogResponse;
    if (!res.ok) {
      return {
        ...data,
        ok: false,
        error: data.error ?? res.statusText,
        diagnostic: data.diagnostic ?? { code: `HTTP_${res.status}`, message: data.error ?? res.statusText },
      };
    }
    return data;
  } catch (error) {
    return {
      ok: false,
      error: 'FETCH_FAILED',
      diagnostic: { code: 'FETCH_FAILED', message: toErrorMessage(error) },
    };
  }
}

export function AgentNavigationDashboard() {
  const [intent, setIntent] = useState(DEFAULT_INTENT);
  const [project, setProject] = useState('missiond');
  const [catalog, setCatalog] = useState<CatalogResponse | null>(null);
  const [guide, setGuide] = useState<CatalogResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [feedbackState, setFeedbackState] = useState<'idle' | 'used' | 'missed'>('idle');

  const selected = guide?.selectedEntry ?? catalog?.selectedEntry ?? null;
  const entries = useMemo(() => catalog?.entries ?? catalog?.projects ?? [], [catalog]);
  const diagnostic = guide?.diagnostic ?? catalog?.diagnostic;
  const routeError = guide?.error ?? catalog?.error;

  const loadCatalog = useCallback(async () => {
    setLoading(true);
    try {
      setCatalog(await fetchNavigation('catalog', project, intent));
    } finally {
      setLoading(false);
    }
  }, [intent, project]);

  const loadGuide = useCallback(async () => {
    setLoading(true);
    try {
      setGuide(await fetchNavigation('guide', project, intent));
    } finally {
      setLoading(false);
    }
  }, [intent, project]);

  const sendFeedback = async (outcome: 'used' | 'missed') => {
    setFeedbackState(outcome);
    try {
      await fetch('/api/agent-navigation', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          project,
          intent,
          entryId: selected?.id,
          outcome,
          agentId: 'board-navigator',
        }),
      });
    } catch (error) {
      setCatalog({
        ok: false,
        error: 'FETCH_FAILED',
        diagnostic: { code: 'FETCH_FAILED', message: toErrorMessage(error) },
      });
    }
    await loadCatalog();
    setFeedbackState('idle');
  };

  useEffect(() => {
    loadCatalog();
  }, [loadCatalog]);

  return (
    <div className="mx-4 mb-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950/60 sm:mx-8">
      <div className="flex flex-col gap-3 border-b border-neutral-800 p-3 lg:flex-row lg:items-center">
        <div className="flex items-center gap-2">
          <Compass className="h-4 w-4 text-orange-300" />
          <div>
            <div className="text-sm font-medium text-neutral-100">Agent Navigator</div>
            <div className="text-[10px] text-neutral-600">{catalog?.review?.eventCount ?? 0} feedback events</div>
          </div>
        </div>
        <div className="flex min-w-0 flex-1 gap-2">
          <input
            value={intent}
            onChange={(event) => setIntent(event.target.value)}
            className="min-w-0 flex-1 rounded-md border border-neutral-800 bg-neutral-950 px-3 py-2 text-xs text-neutral-200 outline-none focus:border-orange-500/60"
            placeholder="Modification intent"
          />
          <input
            value={project}
            onChange={(event) => setProject(event.target.value || 'missiond')}
            className="w-36 rounded-md border border-neutral-800 bg-neutral-950 px-3 py-2 text-xs text-neutral-200 outline-none focus:border-orange-500/60"
            placeholder="project"
          />
        </div>
        <div className="flex gap-2">
          <Button size="sm" variant="outline" onClick={loadCatalog} className="gap-1 border-neutral-800 text-neutral-300">
            {loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Search className="h-3.5 w-3.5" />}
            Catalog
          </Button>
          <Button size="sm" onClick={loadGuide} className="gap-1 bg-orange-500 text-white hover:bg-orange-400">
            <Send className="h-3.5 w-3.5" />
            Guide
          </Button>
        </div>
      </div>

      <div className="grid h-[calc(100%-73px)] min-h-0 grid-cols-1 lg:grid-cols-[320px_1fr]">
        <aside className="min-h-0 overflow-auto border-b border-neutral-800 p-3 lg:border-b-0 lg:border-r">
          <div className="mb-2 text-[10px] uppercase tracking-wide text-neutral-600">Entries</div>
          <div className="space-y-1.5">
            {entries.map((entry) => (
              <button
                key={entry.id ?? entry.projectId}
                onClick={() => setGuide({ ok: true, selectedEntry: entry })}
                className={cn(
                  'w-full rounded-md border px-2 py-2 text-left transition-colors',
                  (selected?.id ?? selected?.projectId) === (entry.id ?? entry.projectId)
                    ? 'border-orange-500/50 bg-orange-500/10 text-orange-100'
                    : 'border-neutral-900 bg-neutral-900/40 text-neutral-400 hover:border-neutral-700 hover:text-neutral-200',
                )}
              >
                <div className="truncate text-xs font-medium">{entry.label ?? entry.id ?? entry.projectId}</div>
                <div className="mt-1 flex gap-2 text-[10px] text-neutral-600">
                  <span>{entry.primaryFamily ?? entry.coverageState ?? 'navigation'}</span>
                  <span>{entry.projectId ?? project}</span>
                </div>
              </button>
            ))}
          </div>
        </aside>

        <main className="min-h-0 overflow-auto p-4">
          {diagnostic ? (
            <div className="mb-3 rounded-md border border-red-500/30 bg-red-500/5 p-3 text-xs text-red-200">
              {diagnostic.code}: {diagnostic.message}
            </div>
          ) : routeError ? (
            <div className="mb-3 rounded-md border border-red-500/30 bg-red-500/5 p-3 text-xs text-red-200">
              {routeError}
            </div>
          ) : null}
          {selected ? (
            <div className="space-y-4">
              <div>
                <div className="text-lg font-semibold text-white">{selected.label ?? selected.id ?? selected.projectId}</div>
                <div className="mt-1 text-xs text-neutral-500">{selected.primaryFamily ?? selected.coverageState ?? 'navigation'}</div>
              </div>
              <ListBlock title="Read First" items={selected.readFirst} />
              <ListBlock title="Checks" items={selected.checks} />
              <ListBlock title="Write Scope" items={selected.writeScope} />
              <ListBlock title="Must Not Touch" items={selected.mustNotTouch} />
              <ListBlock title="Authority" items={selected.authorityNotes} />
              <div className="flex gap-2 pt-2">
                <Button size="sm" variant="outline" onClick={() => sendFeedback('used')} className="gap-1 border-emerald-500/30 text-emerald-200">
                  {feedbackState === 'used' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Check className="h-3.5 w-3.5" />}
                  Used
                </Button>
                <Button size="sm" variant="outline" onClick={() => sendFeedback('missed')} className="gap-1 border-red-500/30 text-red-200">
                  {feedbackState === 'missed' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <X className="h-3.5 w-3.5" />}
                  Missed
                </Button>
              </div>
            </div>
          ) : (
            <div className="text-sm text-neutral-600">No navigation entry selected.</div>
          )}
        </main>
      </div>
    </div>
  );
}

function ListBlock({ title, items }: { title: string; items?: string[] }) {
  const values = items ?? [];
  return (
    <section>
      <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">{title}</div>
      {values.length > 0 ? (
        <ul className="space-y-1">
          {values.map((item) => (
            <li key={item} className="rounded-md border border-neutral-900 bg-neutral-900/35 px-2 py-1.5 font-mono text-[11px] text-neutral-300">
              {item}
            </li>
          ))}
        </ul>
      ) : (
        <div className="text-xs text-neutral-700">None</div>
      )}
    </section>
  );
}
