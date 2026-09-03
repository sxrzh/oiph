import type { ContestData, FileData, ProblemDetail, SessionInfo } from './types';

export async function getContest(): Promise<ContestData> {
  return fetch('/api/contest').then(r => r.json());
}

export async function getProblem(pid: string): Promise<ProblemDetail> {
  return fetch(`/api/problem/${pid}`).then(r => r.json());
}

export async function getFile(path: string): Promise<FileData> {
  return fetch(`/api/file?path=${encodeURIComponent(path)}`).then(r => r.json());
}

export async function saveFile(path: string, content: string): Promise<{ ok?: boolean; error?: string }> {
  return fetch('/api/file', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path, content }),
  }).then(r => r.json());
}

export async function getSessions(): Promise<{ sessions: SessionInfo[]; current: string | null }> {
  return fetch('/api/sessions').then(r => r.json());
}

export async function newSession(name?: string): Promise<{ ok?: boolean; name?: string; error?: string }> {
  return fetch('/api/session/new', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: name ?? null }),
  }).then(r => r.json());
}

export async function switchSession(name: string): Promise<SwitchSessionResp> {
  return fetch('/api/session/switch', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  }).then(r => r.json());
}

export interface SwitchSessionResp {
  ok?: boolean;
  error?: string;
  name?: string;
  messages?: { role: string; content: string | null; tool_calls?: any[] }[];
  children?: { filename: string; agent: string; summary: string }[];
}

export async function exportLemon(): Promise<{ ok?: boolean; path?: string; error?: string }> {
  return fetch('/api/export/lemon', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }).then(r => r.json());
}

export async function runTest(problem?: string): Promise<{ reports: any[] }> {
  return fetch('/api/test', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ problem }) }).then(r => r.json());
}

export type WsMessage =
  | { type: 'content'; text: string }
  | { type: 'reasoning'; text: string }
  | { type: 'log'; text: string }
  | { type: 'tool_call'; name: string; args: any }
  | { type: 'tool_result'; text: string }
  | { type: 'step_boundary' }
  | { type: 'usage'; usage: any }
  | { type: 'done'; interrupted: boolean; usage?: any }
  | { type: 'error'; message: string }
  | { type: 'messages'; messages: any[] }
  | { type: 'session_created'; name: string };
