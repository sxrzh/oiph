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
  usage?: { input: number; output: number; total_tokens?: number; cache_hit_tokens?: number | null };
}

export interface UsageParts {
  input: number;
  output: number;
  hit: number;
}

export interface CostParts {
  currency: string;
  amount: number;
}

export const zeroUsage = (): UsageParts => ({ input: 0, output: 0, hit: 0 });

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
  | { type: 'tool_call'; id: number; name: string; args: any }
  | { type: 'tool_result'; id: number | null; text: string }
  | { type: 'step_boundary'; agent?: string }
  | { type: 'snapshot_done' }
  | { type: 'ask_user'; questions: any[] }
  /** 全局累计用量基线（回合结束/连接建立/切换会话时推送），收到后清空其他两块 */
  | { type: 'usage'; usage: { input: number; output: number; total_tokens?: number; cache_hit_tokens?: number | null; cost?: CostParts | null } }
  /** 本回合已完成的精确用量（turn-local 累计） */
  | { type: 'usage_turn'; usage: { input: number; output: number; cache_hit_tokens?: number | null; cost?: CostParts | null } }
  /** 当前流式调用的增量估算 */
  | { type: 'usage_live'; input: number; output: number }
  | { type: 'done'; interrupted: boolean }
  | { type: 'error'; message: string }
  | { type: 'messages'; messages: any[] }
  | { type: 'session_created'; name: string };
