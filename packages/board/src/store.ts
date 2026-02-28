import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import type { Task, TaskFormData, TaskFiltersState, GroupBy } from './types';
import * as api from './api';

function getDescendantIds(tasks: Task[], parentId: string): string[] {
  const ids: string[] = [];
  const children = tasks.filter((t) => t.parentId === parentId);
  for (const child of children) {
    ids.push(child.id);
    ids.push(...getDescendantIds(tasks, child.id));
  }
  return ids;
}

interface TaskCenterState {
  tasks: Task[];
  isLoading: boolean;
  isSynced: boolean;
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
    filters: { search: '', category: 'all', priority: 'all' },
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
        set({ tasks, isSynced: true });
      } catch (err) {
        console.error('[TaskCenter] Failed to fetch:', err);
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
        description: '',
        status: 'open',
        priority: 'medium',
        category: 'other',
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: maxOrder + 1,
      };
      set({ tasks: [newTask, ...tasks] });

      api.createTask({ title, description: '', priority: 'medium', category: 'other' })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)) });
        })
        .catch((err) => {
          console.error('[TaskCenter] quickAdd sync failed:', err);
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
        description: '',
        status: 'open',
        priority: parent?.priority || 'medium',
        category: parent?.category || 'other',
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
        priority: parent?.priority || 'medium',
        category: parent?.category || 'other',
        project: parent?.project,
        server: parent?.server,
        parentId,
      })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)) });
        })
        .catch((err) => console.error('[TaskCenter] quickAddSub sync failed:', err));
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
        status: 'open',
        description: data.description || '',
        parentId,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: maxOrder + 1,
      };
      set({ tasks: [...tasks, newTask], isDialogOpen: false, editingTask: null, _addDialogParentId: undefined });

      api.createTask({ ...data, description: data.description || '', parentId })
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === tempId ? saved : t)) });
        })
        .catch((err) => console.error('[TaskCenter] addTask sync failed:', err));
    },

    updateTask: (id, data) => {
      set({
        tasks: get().tasks.map((t) =>
          t.id === id ? { ...t, ...data, updatedAt: new Date().toISOString() } : t
        ),
        isDialogOpen: false,
        editingTask: null,
      });

      api.updateTask(id, data)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)) });
        })
        .catch((err) => console.error('[TaskCenter] updateTask sync failed:', err));
    },

    deleteTask: (id) => {
      const { tasks } = get();
      const toDelete = new Set([id, ...getDescendantIds(tasks, id)]);
      set({
        tasks: tasks.filter((t) => !toDelete.has(t.id)),
        isDialogOpen: false,
        editingTask: null,
      });

      api.deleteTask(id)
        .catch((err) => console.error('[TaskCenter] deleteTask sync failed:', err));
    },

    toggleTask: (id) => {
      set({
        tasks: get().tasks.map((t) =>
          t.id === id
            ? { ...t, status: t.status === 'open' ? 'done' as const : 'open' as const, updatedAt: new Date().toISOString() }
            : t
        ),
      });

      api.toggleTask(id)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)) });
        })
        .catch((err) => console.error('[TaskCenter] toggleTask sync failed:', err));
    },

    reorderTask: (taskId, newIndex) => {
      const { tasks } = get();
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
          api.updateTask(id, { orderIdx: order } as never)
        )
      ).catch((err) => console.error('[TaskCenter] reorderTask sync failed:', err));
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
      const newStatus = task.status === 'skipped' ? 'open' : 'skipped';
      set({
        tasks: get().tasks.map((t) =>
          t.id === id ? { ...t, status: newStatus as Task['status'], updatedAt: new Date().toISOString() } : t
        ),
      });
      api.updateTask(id, { status: newStatus } as never)
        .then((saved) => {
          set({ tasks: get().tasks.map((t) => (t.id === id ? saved : t)) });
        })
        .catch((err) => console.error('[TaskCenter] skipTask sync failed:', err));
    },
    openAddDialog: (parentId) => set({ isDialogOpen: true, editingTask: null, _addDialogParentId: parentId }),
    openEditDialog: (task) => set({ isDialogOpen: true, editingTask: task, _addDialogParentId: undefined }),
    closeDialog: () => set({ isDialogOpen: false, editingTask: null, _addDialogParentId: undefined }),

    clearDoneTasks: () => {
      set({ tasks: get().tasks.filter((t) => t.status !== 'done') });
      api.clearDoneTasks()
        .catch((err) => console.error('[TaskCenter] clearDoneTasks sync failed:', err));
    },
  }))
);
