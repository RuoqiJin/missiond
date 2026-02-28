import type { Task, TaskFormData } from './types';

const BASE = '/api/tasks';

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`Tasks API ${res.status}: ${body}`);
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

export async function updateTask(id: string, data: Partial<TaskFormData> & { orderIdx?: number; status?: string }): Promise<Task> {
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
