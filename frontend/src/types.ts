export interface ComponentStatus {
  state: 'not_started' | 'in_progress' | 'completed' | 'failed';
  label: string;
  color: 'gray' | 'yellow' | 'green' | 'red';
  progress?: number;
  message?: string;
  timestamp?: string;
  error?: string;
}

export interface ProblemSummary {
  id: string;
  name: string;
  type: string;
  source: string;
  status: string;
}

export interface Subtask {
  score: number;
  type: string;
  cases: string[];
  pretest: boolean;
  sample: boolean;
  depend: number[];
}

export interface Sol {
  name: string;
  file: string;
  expected_verdict: string;
  expected_score: number | null;
  status: ComponentStatus;
}

export interface AuxFile {
  name: string;
  path: string;
  exists: boolean;
}

export interface ProblemDetail {
  id: string;
  name: string;
  problem_type: string;
  source: string;
  tags: string[];
  time_limit_ms: number;
  memory_limit_mb: number;
  compile_flags: string;
  subtasks: Subtask[];
  data_gen: Record<string, string>;
  statement_status: ComponentStatus;
  std_status: ComponentStatus;
  std_file: string;
  sols: Sol[];
  data_status: ComponentStatus;
  validator_status: ComponentStatus;
  checker_status: ComponentStatus;
  interactive_lib_status: ComponentStatus;
  tutorial_status: ComponentStatus;
  duplicate_check: { found: boolean; matches: string[] } | null;
  last_tested: string | null;
  aux_files: AuxFile[];
}

export interface SessionInfo {
  name: string;
  updated_at: string;
  messages: number;
  current: boolean;
}

export interface ChatMessage {
  role: string;
  content: string | null;
  tool_calls?: { id: string; function: { name: string; arguments: string } }[];
}

export interface ContestData {
  contest_dir: string | null;
  status: string;
  problems: ProblemSummary[];
}

export interface FileData {
  content: string;
  path: string;
  error?: string;
}
