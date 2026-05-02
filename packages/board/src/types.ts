export type TaskStatus = 'open' | 'running' | 'verifying' | 'done' | 'blocked' | 'failed' | 'skipped';
export type TaskPriority = 'high' | 'medium' | 'low';
export type TaskCategory = 'deploy' | 'dev' | 'infra' | 'test' | 'research' | 'diagnosis' | 'investigation' | 'other';
export type GroupBy = 'none' | 'category' | 'priority' | 'project';

export type FlowPhase = 'investigate' | 'consult_gemini_1' | 'plan' | 'consult_gemini_2' | 'execute' | 'finalize' | 'done';

export interface SlotDef {
  id: string;
  label: string;
  role: string;
  running?: boolean;
  state?: string;
  provider?: string;
  engine?: string;
  modelProfile?: string;
  taskClass?: string;
  acceptsBoardTask?: boolean;
  confidence?: number;
  reason?: string;
  activeTool?: string;
  blockedKind?: string;
  latestConversation?: {
    id?: string;
    source?: string;
    title?: string;
    updatedAt?: string;
  } | null;
}

export interface Task {
  id: string;
  title: string;
  description: string;
  status: TaskStatus;
  priority: TaskPriority;
  category: TaskCategory;
  project?: string;
  server?: string;
  dueDate?: string;
  parentId?: string;
  hidden?: boolean;
  createdAt: string;
  updatedAt: string;
  order: number;
  // Task claim fields (conflict prevention)
  claimExecutorId?: string;
  claimExecutorType?: 'pty_slot' | 'manual_session';
  claimedAt?: string;
  // Autopilot fields
  assignee?: string;
  autoExecute?: boolean;
  promptTemplate?: string;
  flowTemplate?: string;
  flowPhase?: FlowPhase;
  dependsOn?: string[];
  leaseExpiresAt?: string;
}

export interface TaskFormData {
  title: string;
  description: string;
  priority: TaskPriority;
  category: TaskCategory;
  project?: string;
  server?: string;
  dueDate?: string;
  hidden?: boolean;
  // Autopilot fields
  assignee?: string;
  autoExecute?: boolean;
  promptTemplate?: string;
  flowTemplate?: string;
  dependsOn?: string[];
}

export interface TaskFiltersState {
  search: string;
  category: TaskCategory | 'all';
  priority: TaskPriority | 'all';
  status: TaskStatus | 'all' | 'active';
}

// ============ Board Task Notes ============

export interface TaskNote {
  id: string;
  taskId: string;
  content: string;
  noteType: 'progress' | 'summary' | 'note';
  author?: string;
  createdAt: string;
}

export interface TaskWithNotes extends Task {
  notes: TaskNote[];
}

// ============ Agent Questions (Pending Decisions) ============

export type QuestionStatus = 'pending' | 'answered' | 'dismissed';

export interface AgentQuestion {
  id: string;
  taskId?: string;
  slotId?: string;
  sessionId?: string;
  question: string;
  context: string;
  status: QuestionStatus;
  answer?: string;
  target: string;           // "user" | "master"
  options?: string;         // JSON array of choices
  decisionType: string;     // architecture/implementation/debug/investigation/risk/preference
  retryCount: number;
  routingTrace?: string;    // JSON: { resolvedTier, path[], latencyMs }
  createdAt: string;
  updatedAt: string;
}

// ============ Decision Engine Types ============

export interface RoutingTrace {
  resolved_tier: string;     // "T1" | "T2" | "T3" | "downgraded" | "error"
  path: TierStep[];
  latency_ms: number;
}

export interface TierStep {
  tier: string;
  status: string;   // "decided" | "missed" | "fast-lane" | "escalated" | "dispatched" | "downgraded"
  reason?: string;
  coverage?: string;
  rule_key?: string;
  action?: string;
  reasoning?: string;
  slot_task_id?: string;
  // Hybrid search telemetry (Tier 1 with embeddings)
  method?: string;               // "hybrid" | "fts_only"
  semantic_similarity?: string;  // e.g. "0.823"
  fts_rank?: number;
  vec_rank?: number;
  rrf_score?: string;            // e.g. "0.0451"
}

export interface DecisionStats {
  total: number;
  answered: number;
  pending: number;
  dismissed: number;
  downgraded: number;
  t1Hits: number;
  t2Hits: number;
  t3Hits: number;
  t1HitRate: string;
  hours: number;
}
