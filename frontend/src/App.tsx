import { useEffect, useRef, useState, useCallback } from 'react';
import { getContest, getProblem, getSessions, switchSession } from './api';
import type { ContestData, ProblemDetail, SessionInfo } from './types';
import type { BudgetInfo, CostParts, UsageParts, WsMessage } from './api';
import { zeroUsage } from './api';
import { MenuBar, StatusBar } from './components/Layout';
import { swAlert } from './components/sw';
import { ProblemArea } from './components/ProblemArea';
import { ChatArea } from './components/ChatArea';
import { SessionBar } from './components/SessionBar';
import { Questionnaire } from './components/Questionnaire';
import type { AskQuestion, AskAnswer } from './components/Questionnaire';

interface DisplayMessage {
  role: string;
  content: string;
  toolCalls?: string;
  agent?: string;
  /** 运行中的工具调用：配对 id + 开始时间（用于"已运行 x 秒"提示） */
  toolId?: number;
  running?: boolean;
  startedAt?: number;
  toolName?: string;
}

interface ChildSession {
  filename: string;
  agent: string;
  summary: string;
}

export default function App() {
  const [contest, setContest] = useState<ContestData | null>(null);
  const [currentProblemId, setCurrentProblemId] = useState<string | null>(null);
  const [problem, setProblem] = useState<ProblemDetail | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [currentSession, setCurrentSession] = useState<string | null>(null);
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [children, setChildren] = useState<ChildSession[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [askQuestions, setAskQuestions] = useState<AskQuestion[] | null>(null);
  const [leftWidth, setLeftWidth] = useState(45);
  // Token 用量三块：全局基线（回合结束/连接/切换时更新）+ 本回合精确累计 + 当前流估算
  const [usageBase, setUsageBase] = useState<UsageParts>(zeroUsage());
  const [usageTurn, setUsageTurn] = useState<UsageParts>(zeroUsage());
  const [usageLive, setUsageLive] = useState<{ input: number; output: number }>({ input: 0, output: 0 });
  // 累计费用（基线 + 回合精确；流估算不计费）
  const [costBase, setCostBase] = useState<CostParts | null>(null);
  const [costTurn, setCostTurn] = useState<CostParts | null>(null);
  // 费用预算（随 usage/usage_turn 消息更新）
  const [budget, setBudget] = useState<BudgetInfo | null>(null);
  const budgetWarnedRef = useRef(false);
  const wsRef = useRef<WebSocket | null>(null);
  const currentMsgRef = useRef<DisplayMessage | null>(null);
  const currentAgentRef = useRef<string>('supervisor');
  const sessionNameRef = useRef<string | null>(null);
  const flushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingFlushRef = useRef(false);

  // 流式期间节流刷新（每 100ms 最多一次重渲染），避免高频 delta 卡死 UI
  const flushNow = useCallback(() => {
    pendingFlushRef.current = false;
    if (flushTimerRef.current) {
      clearTimeout(flushTimerRef.current);
      flushTimerRef.current = null;
    }
    setMessages(m => [...m]);
  }, []);

  const scheduleFlush = useCallback(() => {
    if (pendingFlushRef.current) return;
    pendingFlushRef.current = true;
    flushTimerRef.current = setTimeout(flushNow, 100);
  }, [flushNow]);

  const loadContest = useCallback(async () => {
    const d = await getContest();
    setContest(d);
    if (d.problems.length > 0 && !currentProblemId) {
      setCurrentProblemId(d.problems[0].id);
    }
  }, [currentProblemId]);

  const loadProblem = useCallback(async () => {
    if (!currentProblemId) return;
    const d: any = await getProblem(currentProblemId);
    if (!d.error) setProblem(d);
  }, [currentProblemId]);

  const messagesToDisplay = (raw: { role: string; content: string | null; tool_calls?: any[] }[]): DisplayMessage[] =>
    raw
      .filter((m: any) => m.role !== 'system')
      .map((m: any) => ({
        role: m.role,
        content:
          m.role === 'compaction'
            ? `🔄 上下文已压缩\n\n${m.content || ''}`
            : m.content || '',
        toolCalls: m.tool_calls?.map((tc: any) => `[${tc.function.name}(${tc.function.arguments})]`).join('\n'),
      }));

  const handleMessagesLoaded = (raw: any[], rawChildren: any[], rawUsage?: any) => {
    setMessages(messagesToDisplay(raw));
    setChildren(rawChildren ?? []);
    if (rawUsage) {
      // 切换会话：用量基线重置为新 session 的持久化值，回合/流式清零
      setUsageBase({
        input: rawUsage.input ?? rawUsage.prompt_tokens ?? 0,
        output: rawUsage.output ?? rawUsage.completion_tokens ?? 0,
        hit: rawUsage.cache_hit_tokens ?? 0,
      });
      setUsageTurn(zeroUsage());
      setUsageLive({ input: 0, output: 0 });
      setCostBase(rawUsage.cost ?? null);
      setCostTurn(null);
    }
  };

  // 仅刷新 session 列表（轮询用，不动消息）
  const refreshSessionList = useCallback(async () => {
    const d = await getSessions();
    setSessions(d.sessions);
    setCurrentSession(d.current);
    sessionNameRef.current = d.current;
  }, []);

  // 初始加载：session 列表 + 当前会话历史消息（含持久化用量基线）
  const loadSessions = useCallback(async () => {
    const d = await getSessions();
    setSessions(d.sessions);
    setCurrentSession(d.current);
    sessionNameRef.current = d.current;
    if (d.current) {
      const r = await switchSession(d.current);
      if (r.ok && r.messages) {
        setMessages(messagesToDisplay(r.messages));
        setChildren(r.children ?? []);
      }
    }
  }, []);

  useEffect(() => { loadContest(); }, [loadContest]);
  useEffect(() => { loadProblem(); }, [loadProblem]);
  useEffect(() => { loadSessions(); }, [loadSessions]);
  useEffect(() => {
    const poll = setInterval(() => { loadContest(); refreshSessionList(); }, 5000);
    return () => clearInterval(poll);
  }, [loadContest, refreshSessionList]);

  // WebSocket
  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    wsRef.current = ws;
    ws.onmessage = (e) => {
      const msg: WsMessage = JSON.parse(e.data);
      handleWsMessage(msg);
    };
    ws.onclose = () => setTimeout(() => location.reload(), 2000);
    return () => ws.close();
  }, []);

  const handleWsMessage = (msg: WsMessage) => {
    if (msg.type === 'content') {
      if (!currentMsgRef.current || currentMsgRef.current.role !== 'assistant') {
        currentMsgRef.current = { role: 'assistant', content: '', agent: currentAgentRef.current };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      scheduleFlush();
    } else if (msg.type === 'reasoning') {
      if (!currentMsgRef.current || currentMsgRef.current.role !== 'reasoning') {
        currentMsgRef.current = { role: 'reasoning', content: '', agent: currentAgentRef.current };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      scheduleFlush();
    } else if (msg.type === 'tool_call') {
      const { id, name, args } = msg as any;
      currentMsgRef.current = null;
      setMessages(m => [...m, {
        role: 'tool',
        content: `工具调用：${name}(${JSON.stringify(args, null, 2)})`,
        toolId: id,
        running: true,
        startedAt: Date.now(),
        toolName: name,
      }]);
    } else if (msg.type === 'tool_result') {
      const { id, text } = msg as any;
      currentMsgRef.current = null;
      setMessages(m => {
        // 有配对 id：结束对应工具的运行提示（不追加消息，稍后与结果合并显示）
        const next = id != null
          ? m.map(d => (d.toolId === id ? { ...d, running: false } : d))
          : m;
        return [...next, { role: 'tool', content: text }];
      });
    } else if (msg.type === 'step_boundary') {
      currentAgentRef.current = (msg as any).agent || 'supervisor';
      currentMsgRef.current = null;
      flushNow();
    } else if (msg.type === 'ask_user') {
      setAskQuestions((msg as any).questions);
    } else if (msg.type === 'snapshot_done') {
      // 快照回滚/重做后刷新题目区（文件可能已变化）
      loadProblem();
      loadContest();
    } else if (msg.type === 'usage') {
      // 全局基线更新：清空回合累计与流式估算
      const u = (msg as any).usage;
      setUsageBase({
        input: u.input ?? 0,
        output: u.output ?? 0,
        hit: u.cache_hit_tokens ?? 0,
      });
      setUsageTurn(zeroUsage());
      setUsageLive({ input: 0, output: 0 });
      setCostBase(u.cost ?? null);
      setCostTurn(null);
      if ('budget' in u) setBudget(u.budget ?? null);
    } else if (msg.type === 'usage_turn') {
      // 本回合精确累计（基线之上）
      const u = (msg as any).usage;
      setUsageTurn({
        input: u.input ?? 0,
        output: u.output ?? 0,
        hit: u.cache_hit_tokens ?? 0,
      });
      setUsageLive({ input: 0, output: 0 });
      setCostTurn(u.cost ?? null);
      if ('budget' in u) setBudget(u.budget ?? null);
    } else if (msg.type === 'usage_live') {
      setUsageLive({ input: (msg as any).input ?? 0, output: (msg as any).output ?? 0 });
    } else if (msg.type === 'log') {
      // 其他日志不进对话区
    } else if (msg.type === 'done' || msg.type === 'error') {
      setStreaming(false);
      currentMsgRef.current = null;
      setAskQuestions(null);
      flushNow();
      if (msg.type === 'error') {
        setMessages(m => [...m, { role: 'system', content: '错误：' + msg.message }]);
      } else if ((msg as any).interrupted) {
        setMessages(m => [...m, { role: 'system', content: '已中止' }]);
      }
      // 全局用量随后由 usage 消息推送
    } else if (msg.type === 'session_created') {
      setCurrentSession((msg as any).name);
      sessionNameRef.current = (msg as any).name;
    } else if (msg.type === 'messages') {
      const displayMsgs = messagesToDisplay((msg as any).messages);
      setMessages(displayMsgs);
      if ((msg as any).children) setChildren((msg as any).children);
      if ((msg as any).session_name) {
        sessionNameRef.current = (msg as any).session_name;
        setCurrentSession((msg as any).session_name);
      }
    }
  };

  // 预算警告：首次越过阈值与每次打开页面（初始即为越过）时 sweetalert 提醒
  const overWarn = budget ? budget.limit - budget.used < budget.warn : false;
  useEffect(() => {
    if (overWarn && !budgetWarnedRef.current && budget) {
      budgetWarnedRef.current = true;
      swAlert(
        '费用预算警告',
        `API 费用已接近预算：${budget.used.toFixed(2)} / ${budget.limit} ${budget.currency}`,
      );
    }
    if (!overWarn) budgetWarnedRef.current = false;
  }, [overWarn, budget]);

  const handleSend = (text: string) => {
    setMessages(m => [...m, { role: 'user', content: text }]);
    setStreaming(true);
    // 用量不在这里清零：本回合/上一回合的精确用量会在全局基线（usage 消息）
    // 到达时并入基线并清空，保证统计只增不减
    setUsageLive({ input: 0, output: 0 });
    wsRef.current?.send(JSON.stringify({ type: 'chat', text }));
  };

  const handleStop = () => {
    // 中止对话同时作废问卷
    if (askQuestions) setAskQuestions(null);
    wsRef.current?.send(JSON.stringify({ type: 'stop' }));
  };

  const handleAskSubmit = (answers: AskAnswer[]) => {
    setAskQuestions(null);
    wsRef.current?.send(JSON.stringify({ type: 'ask_answer', answers }));
  };

  const handleAskCancel = () => {
    setAskQuestions(null);
    wsRef.current?.send(JSON.stringify({ type: 'ask_answer', cancelled: true }));
  };

  const handleSplitterMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = leftWidth;
    const onMove = (ev: MouseEvent) => {
      const pct = ((startWidth / 100) * window.innerWidth + (ev.clientX - startX)) / window.innerWidth * 100;
      setLeftWidth(Math.max(20, Math.min(80, pct)));
    };
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

  return (
    <div className="app">
      <MenuBar onTestDone={() => { loadContest(); loadProblem(); }} />
      <div className="main">
        <div className="left-panel" style={{ width: `${leftWidth}%` }}>
          <div className="problem-tabs">
            {contest?.problems.map(p => (
              <div key={p.id} className={`problem-tab ${p.id === currentProblemId ? 'active' : ''}`}
                onClick={() => setCurrentProblemId(p.id)}>
                {p.name || p.id}
              </div>
            ))}
          </div>
          <ProblemArea problem={problem} onRefresh={loadProblem} />
        </div>
        <div className="splitter" onMouseDown={handleSplitterMouseDown} />
        <div className="right-panel">
          <SessionBar sessions={sessions} current={currentSession} onSwitched={refreshSessionList} onMessagesLoaded={handleMessagesLoaded} />
          <ChatArea
            messages={messages}
            childSessions={children}
            sessionName={currentSession}
            streaming={streaming}
            onSend={handleSend}
            onStop={handleStop}
            onUndo={() => wsRef.current?.send(JSON.stringify({ type: 'undo' }))}
            onRedo={() => wsRef.current?.send(JSON.stringify({ type: 'redo' }))}
          >
            {askQuestions && (
              <Questionnaire
                questions={askQuestions}
                onSubmit={handleAskSubmit}
                onCancel={handleAskCancel}
              />
            )}
          </ChatArea>
        </div>
      </div>
      {/* 显示用量 = 全局基线 + 本回合精确 + 当前流估算（只增不减） */}
      <StatusBar
        path={contest?.contest_dir ?? '无比赛工程'}
        usage={{
          input: usageBase.input + usageTurn.input + usageLive.input,
          output: usageBase.output + usageTurn.output + usageLive.output,
          hit: usageBase.hit + usageTurn.hit,
          cost: (costBase || costTurn)
            ? {
                currency: (costTurn ?? costBase)!.currency,
                amount: (costBase?.amount ?? 0) + (costTurn?.amount ?? 0),
              }
            : null,
          budget,
        }}
      />
    </div>
  );
}
