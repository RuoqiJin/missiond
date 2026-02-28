export type TaskStatus = 'open' | 'running' | 'verifying' | 'done' | 'blocked' | 'failed' | 'skipped';
export type TaskPriority = 'high' | 'medium' | 'low';
export type TaskCategory = 'deploy' | 'dev' | 'infra' | 'test' | 'other';
export type GroupBy = 'none' | 'category' | 'priority' | 'project';

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
}

export interface TaskFiltersState {
  search: string;
  category: TaskCategory | 'all';
  priority: TaskPriority | 'all';
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
  createdAt: string;
  updatedAt: string;
}
