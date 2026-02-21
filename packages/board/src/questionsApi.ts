import type { AgentQuestion } from './types';

const BASE = '/api/questions';

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`Questions API ${res.status}: ${body}`);
  }
  return res.json();
}

export async function fetchQuestions(status?: string): Promise<AgentQuestion[]> {
  const params = status ? `?status=${status}` : '';
  return request<AgentQuestion[]>(`${BASE}${params}`);
}

export async function answerQuestion(id: string, answer: string): Promise<AgentQuestion> {
  return request<AgentQuestion>(`${BASE}?action=answer&id=${encodeURIComponent(id)}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ answer }),
  });
}

export async function dismissQuestion(id: string): Promise<AgentQuestion> {
  return request<AgentQuestion>(`${BASE}?action=dismiss&id=${encodeURIComponent(id)}`, {
    method: 'POST',
  });
}
