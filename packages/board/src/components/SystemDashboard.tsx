'use client';

import { useState, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, Activity, Gauge, Network, ServerCog, ShieldCheck } from 'lucide-react';
import { cn } from '@/lib/utils';
import { MemoryDashboard } from './MemoryDashboard';
import { EngineDashboard } from './EngineDashboard';
import { DecisionDashboard } from './DecisionDashboard';

function CollapsibleSection({
  title,
  icon: Icon,
  iconColor,
  expanded,
  onToggle,
  children,
}: {
  title: string;
  icon: React.ComponentType<{ className?: string }>;
  iconColor: string;
  expanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col min-h-0">
      <button
        onClick={onToggle}
        className="flex items-center gap-2 px-4 sm:px-8 py-2.5 text-sm font-medium text-neutral-300 hover:text-white transition-colors border-b border-neutral-800/50 shrink-0"
      >
        {expanded ? (
          <ChevronDown className="w-4 h-4 text-neutral-500" />
        ) : (
          <ChevronRight className="w-4 h-4 text-neutral-500" />
        )}
        <Icon className={cn('w-4 h-4', iconColor)} />
        {title}
      </button>
      {expanded && (
        <div className="flex-1 min-h-0 overflow-auto">
          {children}
        </div>
      )}
    </div>
  );
}

interface ProjectInfo {
  id: string;
  path: string;
  intentPath?: string;
  active: boolean;
  slots: string[];
  githubUrl?: string;
  lispFiles?: string[];
  lispCount?: number;
  runtimeServices?: RuntimeService[];
}

interface RuntimeService {
  id: string;
  environment?: string;
  publicBaseUrl?: string;
  issuer?: string;
  domains?: string[];
  dnsProvider?: string;
  dnsCapability?: string;
  opsCapability?: string;
  root?: string;
  deployment?: {
    substrate?: string;
    namespace?: string;
    deployment?: string;
    service?: string;
    replicas?: string;
    hpaMin?: string;
    hpaMax?: string;
    image?: string;
  };
  proxy?: {
    kind?: string;
    domain?: string;
    file?: string;
    sseNoBuffer?: string;
  };
  health?: string[];
  dependencies?: string[];
  risks?: string[];
}

interface InfraTarget {
  id: string;
  name: string;
  provider: string;
  host?: string;
  lan?: string;
  location?: string;
  roles?: string[];
  tags?: string[];
  healthEndpoint?: string;
}

interface CredentialRef {
  targetId?: string;
  namespace?: string;
  keyName?: string;
  secretRef?: string;
  purpose?: string;
  requiredCapability?: string;
  availability?: string;
}

interface SkillEvidence {
  sourceSkill?: string;
  sourcePath?: string;
  sourceLine?: number;
  confidence?: string;
  promoteTo?: string;
  credentialInlineRisk?: boolean;
  excerpt?: string;
}

interface DiagnosticProfile {
  targetId?: string;
  serviceId?: string;
  profileId?: string;
  authority?: string;
  readOnly?: boolean;
  requiredExecutor?: string;
  canExecuteFromMissionD?: boolean;
}

interface InfraPayload {
  targets: InfraTarget[];
  health?: {
    runtimeTargets?: number;
    skillEvidenceItems?: number;
    credentialInlineRisks?: number;
  };
  credentialRefs?: { credentialRefs?: CredentialRef[] };
  diagnosticProfiles?: { profiles?: DiagnosticProfile[] };
  skillEvidence?: { items?: SkillEvidence[] };
  reconcile?: {
    consistent?: boolean;
    drift?: Record<string, unknown>;
  };
}

function ProjectsPanel() {
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchProjects = useCallback(async () => {
    try {
      const res = await fetch('/api/projects');
      const data = await res.json();
      if (!Array.isArray(data)) return;
      // Sort: active first, then by lispCount desc, then alphabetical
      const sorted = data.sort((a: ProjectInfo, b: ProjectInfo) => {
        if (a.active !== b.active) return a.active ? -1 : 1;
        return (b.lispCount ?? 0) - (a.lispCount ?? 0) || a.id.localeCompare(b.id);
      });
      setProjects(sorted);
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchProjects(); }, [fetchProjects]);

  if (loading) {
    return <div className="px-6 py-4 text-xs text-neutral-500">加载中...</div>;
  }

  const activeProjects = projects.filter((p) => p.active);
  const inactiveProjects = projects.filter((p) => !p.active);

  return (
    <div className="px-4 sm:px-8 py-4 space-y-3">
      <div className="flex items-center gap-3 text-xs text-neutral-500">
        <span>{projects.length} 个项目</span>
        <span>·</span>
        <span className="text-emerald-500">{activeProjects.length} active</span>
        <span className="text-neutral-600">{inactiveProjects.length} inactive</span>
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {projects.map((p) => (
          <div
            key={p.id}
            className={cn(
              'border rounded-lg p-3 transition-colors',
              p.active
                ? 'border-neutral-800 hover:border-neutral-700 bg-neutral-900/50'
                : 'border-neutral-800/50 bg-neutral-950/50 opacity-60',
            )}
          >
            <div className="flex items-center justify-between mb-1.5">
              <span className="text-sm font-medium text-neutral-200">{p.id}</span>
              <span
                className={cn(
                  'px-1.5 py-0.5 text-[10px] rounded-full font-medium',
                  p.active
                    ? 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20'
                    : 'bg-neutral-800 text-neutral-500 border border-neutral-700',
                )}
              >
                {p.active ? 'active' : 'inactive'}
              </span>
            </div>
            <div className="text-[11px] text-neutral-500 truncate mb-2" title={p.path}>
              {p.path.replace(/^\/Users\/jinchen\//, '~/')}
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] mb-2">
              {p.githubUrl && (
                <a
                  href={p.githubUrl.replace(/^git@github\.com:/, 'https://github.com/').replace(/\.git$/, '')}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-blue-400/80 hover:text-blue-300 transition-colors"
                  onClick={(e) => e.stopPropagation()}
                >
                  ↗ GitHub
                </a>
              )}
              {p.slots.length > 0 && (
                <span className="text-cyan-500/70">
                  ⊞ {p.slots.join(', ')}
                </span>
              )}
            </div>
            {(p.lispCount ?? 0) > 0 && (
              <div className="text-[11px] space-y-0.5">
                <span className="text-amber-500/70">{p.lispCount} lisp</span>
                <div className="flex flex-wrap gap-1">
                  {(p.lispFiles ?? []).map((f) => (
                    <span key={f} className="px-1.5 py-0.5 rounded bg-amber-500/5 text-amber-500/50 text-[10px] border border-amber-500/10">
                      {f}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {p.runtimeServices?.length ? (
              <div className="mt-3 space-y-2 border-t border-neutral-800/70 pt-2">
                {p.runtimeServices.map((svc) => (
                  <ServiceRuntimeCard key={svc.id} service={svc} />
                ))}
              </div>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}

function InfraPanel() {
  const [infra, setInfra] = useState<InfraPayload | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchInfra = useCallback(async () => {
    try {
      const res = await fetch('/api/infra');
      const data = await res.json();
      setInfra(data);
    } catch {
      setInfra(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchInfra(); }, [fetchInfra]);

  if (loading) {
    return <div className="px-6 py-4 text-xs text-neutral-500">加载中...</div>;
  }

  const targets = infra?.targets ?? [];
  const refs = infra?.credentialRefs?.credentialRefs ?? [];
  const profiles = infra?.diagnosticProfiles?.profiles ?? [];
  const evidence = infra?.skillEvidence?.items ?? [];
  const risks = evidence.filter((item) => item.credentialInlineRisk);

  return (
    <div className="px-4 sm:px-8 py-4 space-y-4">
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <MetricCard label="runtime targets" value={String(targets.length)} />
        <MetricCard label="diagnostic profiles" value={String(profiles.length)} />
        <MetricCard label="skill evidence" value={String(infra?.health?.skillEvidenceItems ?? evidence.length)} />
        <MetricCard label="credential risks" value={String(infra?.health?.credentialInlineRisks ?? risks.length)} tone={risks.length ? 'warn' : 'ok'} />
        <MetricCard label="reconcile" value={infra?.reconcile?.consistent ? 'clean' : 'drift'} tone={infra?.reconcile?.consistent ? 'ok' : 'warn'} />
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-3 gap-3">
        <div className="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
          <div className="mb-2 text-xs font-medium text-neutral-300">Runtime Targets</div>
          <div className="space-y-2">
            {targets.map((target) => (
              <div key={target.id} className="rounded-md border border-neutral-800 bg-neutral-900/50 p-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-xs font-medium text-neutral-200">{target.id}</span>
                  <span className={cn(
                    'rounded border px-1.5 py-0.5 text-[9px]',
                    target.provider === 'skill-derived'
                      ? 'border-amber-500/20 bg-amber-500/5 text-amber-300'
                      : 'border-emerald-500/20 bg-emerald-500/5 text-emerald-300',
                  )}>
                    {target.provider}
                  </span>
                </div>
                <div className="mt-1 text-[11px] text-neutral-500">{target.name}</div>
                <div className="mt-2 grid grid-cols-2 gap-x-2 gap-y-1 text-[10px] text-neutral-500">
                  <RuntimeKV label="host" value={target.host || target.lan} />
                  <RuntimeKV label="location" value={target.location} />
                  <RuntimeKV label="health" value={target.healthEndpoint} />
                  <RuntimeKV label="roles" value={(target.roles ?? []).slice(0, 3).join(', ')} />
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
          <div className="mb-2 text-xs font-medium text-neutral-300">Read-only Diagnostic Profiles</div>
          <div className="space-y-2">
            {profiles.slice(0, 10).map((profile) => (
              <div key={`${profile.targetId}-${profile.profileId}`} className="rounded-md border border-neutral-800 bg-neutral-900/50 p-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-xs text-neutral-300">{profile.targetId}</span>
                  <span className="text-[9px] text-neutral-500">{profile.profileId}</span>
                </div>
                <div className="mt-1 text-[10px] text-neutral-500">{profile.requiredExecutor}</div>
                <div className="mt-1 text-[10px] text-neutral-500">
                  authority {profile.authority || 'deploy-center'} · MissionD direct exec {profile.canExecuteFromMissionD ? 'yes' : 'no'}
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
          <div className="mb-2 text-xs font-medium text-neutral-300">Credential Refs</div>
          <div className="space-y-2">
            {refs.map((ref) => (
              <div key={`${ref.targetId}-${ref.secretRef}`} className="rounded-md border border-neutral-800 bg-neutral-900/50 p-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-xs text-neutral-300">{ref.targetId}</span>
                  <span className="text-[9px] text-neutral-500">{ref.availability || 'unknown'}</span>
                </div>
                <div className="mt-1 truncate text-[11px] text-cyan-300" title={ref.secretRef}>{ref.secretRef}</div>
                <div className="mt-1 text-[10px] text-neutral-500">{ref.purpose}</div>
              </div>
            ))}
          </div>
          {risks.length ? (
            <div className="mt-4 rounded-md border border-amber-500/20 bg-amber-500/5 p-2">
              <div className="text-xs font-medium text-amber-300">Skill credential-like evidence</div>
              <div className="mt-2 space-y-1">
                {risks.slice(0, 5).map((item) => (
                  <div key={`${item.sourceSkill}-${item.sourceLine}`} className="text-[10px] text-amber-200/70">
                    {item.sourceSkill}:{item.sourceLine} → {item.promoteTo}
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

export function SystemDashboard() {
  const [projectsExpanded, setProjectsExpanded] = useState(true);
  const [decisionsExpanded, setDecisionsExpanded] = useState(true);
  const [infraExpanded, setInfraExpanded] = useState(true);
  const [memoryExpanded, setMemoryExpanded] = useState(true);
  const [engineExpanded, setEngineExpanded] = useState(true);

  return (
    <div className="flex-1 flex flex-col overflow-auto">
      <CollapsibleSection
        title="SSOT Universe"
        icon={Network}
        iconColor="text-cyan-400"
        expanded={projectsExpanded}
        onToggle={() => setProjectsExpanded((v) => !v)}
      >
        <ProjectsPanel />
      </CollapsibleSection>

      <CollapsibleSection
        title="Infrastructure Universe"
        icon={ShieldCheck}
        iconColor="text-emerald-400"
        expanded={infraExpanded}
        onToggle={() => setInfraExpanded((v) => !v)}
      >
        <InfraPanel />
      </CollapsibleSection>

      <CollapsibleSection
        title="Decision Inbox"
        icon={ServerCog}
        iconColor="text-orange-400"
        expanded={decisionsExpanded}
        onToggle={() => setDecisionsExpanded((v) => !v)}
      >
        <div className="h-[520px] min-h-0 py-4">
          <DecisionDashboard />
        </div>
      </CollapsibleSection>

      <CollapsibleSection
        title="Memory Review"
        icon={Activity}
        iconColor="text-purple-400"
        expanded={memoryExpanded}
        onToggle={() => setMemoryExpanded((v) => !v)}
      >
        <MemoryDashboard />
      </CollapsibleSection>

      <CollapsibleSection
        title="Runtime Health"
        icon={Gauge}
        iconColor="text-blue-400"
        expanded={engineExpanded}
        onToggle={() => setEngineExpanded((v) => !v)}
      >
        <EngineDashboard />
      </CollapsibleSection>
    </div>
  );
}

function MetricCard({ label, value, tone = 'neutral' }: { label: string; value: string; tone?: 'neutral' | 'ok' | 'warn' }) {
  return (
    <div className="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
      <div className="text-[10px] uppercase tracking-wide text-neutral-600">{label}</div>
      <div className={cn(
        'mt-1 text-lg font-semibold',
        tone === 'ok' && 'text-emerald-300',
        tone === 'warn' && 'text-amber-300',
        tone === 'neutral' && 'text-neutral-200',
      )}>
        {value}
      </div>
    </div>
  );
}

function ServiceRuntimeCard({ service }: { service: RuntimeService }) {
  const deploy = service.deployment;
  const risks = service.risks ?? [];
  return (
    <div className="rounded-md border border-cyan-500/10 bg-cyan-500/[0.03] p-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium text-cyan-300">{service.id}</span>
        <span className="rounded border border-cyan-500/20 bg-cyan-500/10 px-1.5 py-0.5 text-[9px] text-cyan-300">
          {service.environment || 'runtime'}
        </span>
      </div>
      <div className="mt-1 truncate text-[11px] text-neutral-300" title={service.publicBaseUrl || service.domains?.[0]}>
        {service.publicBaseUrl || service.domains?.[0] || 'no public domain'}
      </div>
      <div className="mt-2 grid grid-cols-2 gap-x-2 gap-y-1 text-[10px] text-neutral-500">
        <RuntimeKV label="deploy" value={[deploy?.substrate, deploy?.namespace, deploy?.deployment].filter(Boolean).join(' / ')} />
        <RuntimeKV label="service" value={deploy?.service} />
        <RuntimeKV label="dns" value={[service.dnsProvider, service.opsCapability].filter(Boolean).join(' / ')} />
        <RuntimeKV label="proxy" value={[service.proxy?.kind, service.proxy?.domain].filter(Boolean).join(' / ')} />
      </div>
      {service.health?.length ? (
        <div className="mt-2 flex flex-wrap gap-1">
          {service.health.slice(0, 4).map((h) => (
            <span key={h} className="rounded bg-neutral-900 px-1.5 py-0.5 text-[9px] text-neutral-500">{h}</span>
          ))}
        </div>
      ) : null}
      {risks.length ? (
        <div className="mt-2 flex flex-wrap gap-1">
          {risks.map((risk) => (
            <span key={risk} className="rounded border border-amber-500/20 bg-amber-500/5 px-1.5 py-0.5 text-[9px] text-amber-300">{risk}</span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function RuntimeKV({ label, value }: { label: string; value?: string }) {
  return (
    <div className="min-w-0">
      <span className="mr-1 text-neutral-600">{label}</span>
      <span className="text-neutral-400" title={value || undefined}>{value || '-'}</span>
    </div>
  );
}
