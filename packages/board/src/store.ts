import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import type { Task, TaskFormData, TaskFiltersState, GroupBy } from './types';
import * as api from './api';
import { BOARD_TASK_DEFAULTS } from './generated/board-frontend-config';

function getDescendantIds(tasks: Task[], parentId: string): string[] {
  const ids: string[] = [];
  const children = tasks.filter((t) => t.parentId === parentId);
  for (const child of children) {
    ids.push(child.id);
    ids.push(...getDescendantIds(tasks, child.id));
  }
  return ids;
}

function syncErrorMessage(err: unknown): string {
  if (err && typeof err === 'object') {
    const body = err as { code?: string; message?: string; suggestedAction?: string; error_code?: string; reason?: string; suggestion?: string };
    const code = body.code ?? body.error_code;
    if (code === 'EVIDENCE_REQUIRED') {
      return `${body.message ?? body.reason ?? 'Completion evidence is required.'} Suggested action: ${body.suggestedAction ?? body.suggestion ?? 'mission_shared_memory(action="task_result_put", ...)'}`;
    }
    if (code === 'CLAIM_CONFLICT') {
      return body.message ?? body.reason ?? 'Claim conflict: another worker already owns this scope.';
    }
    if (code === 'CAPABILITY_DENIED') {
      return body.message ?? body.reason ?? 'Capability denied: this task does not carry an active grant for the requested operation.';
    }
    if (code === 'WRITE_SCOPE_VIOLATION') {
      return body.message ?? body.reason ?? 'Write scope violation: changes escaped the delegated write_scope or touched a forbidden path.';
    }
    if (code === 'RUNTIME_METADATA_REQUIRED') {
      return body.message ?? body.reason ?? 'Runtime metadata is required before this task can drive control-plane state.';
    }
    if (body.message || body.reason) return body.message ?? body.reason ?? '';
  }
  return err instanceof Error ? err.message : String(err);
}

interface TaskCenterState {
  tasks: Task[];
  isLoading: boolean;
  isSynced: boolean;
  lastError: string | null;
  filters: TaskFiltersState;
  groupBy: GroupBy;
  showDone: boolean;
  editingTask: Task | null;
  isDialogOpen: boolean;
  _addDialogParentId?: string;

  fetchTasks: () => Promise<void>;
  addTask: (data: TaskFormData, parentId?: string) => void;
  quickAdd: (title: string) => void;
  quickAddSub: (parentId: string, title: string) => void;
  updateTask: (id: string, data: Partial<TaskFormData>) => void;
  deleteTask: (id: string) => void;
  toggleTask: (id: string) => void;
  reorderTask: (taskId: string, newIndex: number) => void;
  clearDoneTasks: () => void;
  clearLastError: () => void;

  setFilters: (filters: Partial<TaskFiltersState>) => void;
  setGroupBy: (groupBy: GroupBy) => void;
  setShowDone: (show: boolean) => void;
  showHidden: boolean;
  setShowHidden: (show: boolean) => void;
  showSkipped: boolean;
  setShowSkipped: (show: boolean) => void;
  skipTask: (id: string) => void;
  openAddDialog: (parentId?: string) => void;
  openEditDialog: (task: Task) => void;
  closeDialog: () => void;
}

export const useTaskCenterStore = create<TaskCenterState>()(
  subscribeWithSelector((set, get) => ({
    tasks: [],
    isLoading: false,
    isSynced: false,
    lastError: null,
    filters: { search: '', category: 'all', priority: 'all', status: 'active' },
    groupBy: 'category',
    showDone: false,
    showHidden: false,
    editingTask: null,
    isDialogOpen: false,
    _addDialogParentId: undefined,

    fetchTasks: async () => {
      set({ isLoading: true });
      try {
        const tasks = await api.fetchTasks();
        set({ tasks, isSynced: true, lastError: null });
      } catch (err) {
        console.error('[TaskCenter] Failed to fetch:', err);
        set({ lastError: syncErrorMessage(err), isSynced: false });
      } finally {
        set({ isLoading: false });
      }
    },

    quickAdd: (title) => {
      const { tasks } = get();
      const maxOrder = tasks.filter((t) => !t.parentId).reduce((max, t) => Math.max(max, t.order), -1);
      const tempId = `temp-${Date.now()}`;
      const newTask: Task = {
        id: tempId,
        title,
        description: BOARD_TASK_DEFAULTS.description,
        status: BOARD_TASK_DEFAULTS.status,
        priority: BOARD_TASK_DEFAULTS.priority,
        category: BOARD_TASK_DEFAULTS.category,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: maxOrder + 1,
      };
      set({ tasks: [newTask, ...tasks] });

      api.createTask({ title, description: BOARD_TASK_DEFAULTS.description, priority: BOARD_TASK_DEFAULTS.priority, category: BOARD_TASK_DEFAULTS.category })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] quickAdd sync failed:', err);
          set({ tasks: get().tasks.filter((t) => t.id !== tempId), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    quickAddSub: (parentId, title) => {
      const { tasks } = get();
      const parent = tasks.find((t) => t.id === parentId);
      const siblings = tasks.filter((t) => t.parentId === parentId);
      const maxOrder = siblings.reduce((max, t) => Math.max(max, t.order), -1);
      const tempId = `temp-${Date.now()}`;
      const newTask: Task = {
        id: tempId,
        title,
        description: BOARD_TASK_DEFAULTS.description,
        status: BOARD_TASK_DEFAULTS.status,
        priority: parent?.priority || BOARD_TASK_DEFAULTS.priority,
        category: parent?.category || BOARD_TASK_DEFAULTS.category,
        project: parent?.project,
        server: parent?.server,
        parentId,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: maxOrder + 1,
      };
      set({ tasks: [...tasks, newTask] });

      api.createTask({
        title,
        description: '',
        priority: parent?.priority || BOARD_TASK_DEFAULTS.priority,
        category: parent?.category || BOARD_TASK_DEFAULTS.category,
        project: parent?.project,
        server: parent?.server,
        parentId,
      })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] quickAddSub sync failed:', err);
          set({ tasks: get().tasks.filter((t) => t.id !== tempId), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    addTask: (data, parentId) => {
      const { tasks } = get();
      const scope = parentId
        ? tasks.filter((t) => t.parentId === parentId)
        : tasks.filter((t) => !t.parentId);
      const maxOrder = scope.reduce((max, t) => Math.max(max, t.order), -1);
      const tempId = `temp-${Date.now()}`;
      const newTask: Task = {
        ...data,
        id: tempId,
        status: BOARD_TASK_DEFAULTS.status,
        description: data.description || BOARD_TASK_DEFAULTS.description,
        parentId,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: maxOrder + 1,
      };
      set({ tasks: [...tasks, newTask], isDialogOpen: false, editingTask: null, _addDialogParentId: undefined });

      api.createTask({ ...data, description: data.description || BOARD_TASK_DEFAULTS.description, parentId })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] addTask sync failed:', err);
          set({ tasks: get().tasks.filter((t) => t.id !== tempId), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    updateTask: (id, data) => {
      const previousTask = get().tasks.find((t) => t.id === id);
      if (!previousTask) return;
      set({
        tasks: get().tasks.map((t) =>
          t.id === id ? { ...t, ...data, updatedAt: new Date().toISOString() } : t
        ),
        isDialogOpen: false,
        editingTask: null,
      });

      api.updateTask(id, data)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] updateTask sync failed:', err);
          set({ tasks: get().tasks.map((t) => (t.id === id ? previousTask : t)), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    deleteTask: (id) => {
      const { tasks } = get();
      const previousTasks = tasks;
      const toDelete = new Set([id, ...getDescendantIds(tasks, id)]);
      set({
        tasks: tasks.filter((t) => !toDelete.has(t.id)),
        isDialogOpen: false,
        editingTask: null,
      });

      api.deleteTask(id)
        .catch((err) => {
          console.error('[TaskCenter] deleteTask sync failed:', err);
          set({ tasks: previousTasks, isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    toggleTask: (id) => {
      const previousTask = get().tasks.find((t) => t.id === id);
      if (!previousTask) return;
      set({
        tasks: get().tasks.map((t) =>
          t.id === id
            ? { ...t, status: t.status === BOARD_TASK_DEFAULTS.status ? 'done' as const : BOARD_TASK_DEFAULTS.status, updatedAt: new Date().toISOString() }
            : t
        ),
      });

      api.toggleTask(id)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] toggleTask sync failed:', err);
          set({ tasks: get().tasks.map((t) => (t.id === id ? previousTask : t)), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },

    reorderTask: (taskId, newIndex) => {
      const { tasks } = get();
      const previousTasks = tasks;
      const task = tasks.find((t) => t.id === taskId);
      if (!task) return;
      const ACTIVE = new Set(['open', 'running', 'verifying', 'blocked', 'failed']);
      const siblings = tasks
        .filter((t) => t.parentId === task.parentId && ACTIVE.has(t.status))
        .sort((a, b) => a.order - b.order);
      const oldIndex = siblings.findIndex((t) => t.id === taskId);
      if (oldIndex === -1 || oldIndex === newIndex) return;

      const reordered = [...siblings];
      const [moved] = reordered.splice(oldIndex, 1);
      reordered.splice(newIndex, 0, moved);

      const updatedIds = new Map(reordered.map((t, i) => [t.id, i]));
      set({
        tasks: tasks.map((t) => updatedIds.has(t.id) ? { ...t, order: updatedIds.get(t.id)! } : t),
      });

      // Persist all affected siblings, not just the moved one
      Promise.all(
        Array.from(updatedIds.entries()).map(([id, order]) =>
          api.updateTask(id, { order })
        )
      ).catch((err) => {
        console.error('[TaskCenter] reorderTask sync failed:', err);
        set({ tasks: previousTasks, isSynced: false, lastError: syncErrorMessage(err) });
      });
    },

    setFilters: (partial) => set({ filters: { ...get().filters, ...partial } }),
    setGroupBy: (groupBy) => set({ groupBy }),
    setShowDone: (showDone) => set({ showDone }),
    setShowHidden: (showHidden) => set({ showHidden }),
    showSkipped: false,
    setShowSkipped: (showSkipped) => set({ showSkipped }),
    skipTask: (id) => {
      const task = get().tasks.find((t) => t.id === id);
      if (!task) return;
      const previousTask = task;
      const newStatus = task.status === 'skipped' ? BOARD_TASK_DEFAULTS.status : 'skipped';
      set({
        tasks: get().tasks.map((t) =>
          t.id === id ? { ...t, status: newStatus as Task['status'], updatedAt: new Date().toISOString() } : t
        ),
      });
      api.updateTask(id, { status: newStatus } as never)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)), lastError: null });
        })
        .catch((err) => {
          console.error('[TaskCenter] skipTask sync failed:', err);
          set({ tasks: get().tasks.map((t) => (t.id === id ? previousTask : t)), isSynced: false, lastError: syncErrorMessage(err) });
        });
    },
    clearLastError: () => set({ lastError: null }),
    openAddDialog: (parentId) => set({ isDialogOpen: true, editingTask: null, _addDialogParentId: parentId }),
    openEditDialog: (task) => set({ isDialogOpen: true, editingTask: task, _addDialogParentId: undefined }),
    closeDialog: () => set({ isDialogOpen: false, editingTask: null, _addDialogParentId: undefined }),

    clearDoneTasks: () => {
      const previousTasks = get().tasks;
      set({ tasks: get().tasks.filter((t) => t.status !== 'done') });
      api.clearDoneTasks()
        .catch((err) => {
          console.error('[TaskCenter] clearDoneTasks sync failed:', err);
          set({ tasks: previousTasks, isSynced: false, lastError: syncErrorMessage(err) });
        });
    },
  }))
);
