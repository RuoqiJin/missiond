export type TaskStatus = 'open' | 'done';
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
  createdAt: string;
  updatedAt: string;
  order: number;
}

export interface TaskFormData {
  title: string;
  description: string;
  priority: TaskPriority;
  category: TaskCategory;
  project?: string;
  server?: string;
  dueDate?: string;
}

export interface TaskFiltersState {
  search: string;
  category: TaskCategory | 'all';
  priority: TaskPriority | 'all';
}
