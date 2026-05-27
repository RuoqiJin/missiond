import type { Task, TaskFormData, TaskNote, TaskWithNotes } from './types';
import { BOARD_TASK_API_ROUTE } from './generated/board-frontend-config';

const BASE = BOARD_TASK_API_ROUTE;

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    try {
      const parsed = JSON.parse(body);
      throw parsed;
    } catch (err) {
      if (err && typeof err === 'object' && 'code' in err) throw err;
      throw new Error(`Tasks API ${res.status}: ${body}`);
    }
  }
  return res.json();
}

export async function fetchTasks(status?: string): Promise<Task[]> {
  const params = status ? `?status=${status}` : '';
  return request<Task[]>(`${BASE}${params}`);
}

export async function createTask(data: TaskFormData & { parentId?: string }): Promise<Task> {
  return request<Task>(BASE, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
}

export async function updateTask(id: string, data: Partial<TaskFormData> & { order?: number; status?: string }): Promise<Task> {
  return request<Task>(`${BASE}?id=${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
}

export async function deleteTask(id: string): Promise<{ deleted: number }> {
  return request<{ deleted: number }>(`${BASE}?id=${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export async function toggleTask(id: string): Promise<Task> {
  return request<Task>(`${BASE}?action=toggle&id=${encodeURIComponent(id)}`, {
    method: 'POST',
  });
}

export async function clearDoneTasks(): Promise<{ deleted: number }> {
  return request<{ deleted: number }>(`${BASE}?action=clear-done`, {
    method: 'POST',
  });
}

export async function fetchTaskWithNotes(id: string): Promise<TaskWithNotes> {
  return request<TaskWithNotes>(`${BASE}?id=${encodeURIComponent(id)}`);
}

export async function addTaskNote(taskId: string, content: string): Promise<TaskNote> {
  return request<TaskNote>(`${BASE}?action=add-note&id=${encodeURIComponent(taskId)}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content, noteType: 'note' }),
  });
}
