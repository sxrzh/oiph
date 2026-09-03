import { useEffect, useRef, useState, useCallback } from 'react';
import { getContest, getProblem, getSessions, switchSession } from './api';
import type { ContestData, ProblemDetail, SessionInfo } from './types';
import { MenuBar, StatusBar } from './components/Layout';
import { ProblemArea } from './components/ProblemArea';
import { ChatArea } from './components/ChatArea';
import { SessionBar } from './components/SessionBar';
import type { WsMessage } from './api';

interface DisplayMessage {
  role: string;
  content: string;
  toolCalls?: string;
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
  const [usage, setUsage] = useState<any>(null);
  const [leftWidth, setLeftWidth] = useState(45);
  const wsRef = useRef<WebSocket | null>(null);
  const currentMsgRef = useRef<DisplayMessage | null>(null);
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
        content: m.content || '',
        toolCalls: m.tool_calls?.map((tc: any) => `[${tc.function.name}(${tc.function.arguments})]`).join('\n'),
      }));

  const handleMessagesLoaded = (raw: any[], rawChildren: any[]) => {
    setMessages(messagesToDisplay(raw));
    setChildren(rawChildren ?? []);
  };

  // 仅刷新 session 列表（轮询用，不动消息）
  const refreshSessionList = useCallback(async () => {
    const d = await getSessions();
    setSessions(d.sessions);
    setCurrentSession(d.current);
    sessionNameRef.current = d.current;
  }, []);

  // 初始加载：session 列表 + 当前会话历史消息
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
        currentMsgRef.current = { role: 'assistant', content: '' };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      scheduleFlush();
    } else if (msg.type === 'reasoning') {
      if (!currentMsgRef.current || currentMsgRef.current.role !== 'reasoning') {
        currentMsgRef.current = { role: 'reasoning', content: '' };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      scheduleFlush();
    } else if (msg.type === 'tool_call') {
      const { name, args } = msg as any;
      setMessages(m => [...m, { role: 'tool', content: `工具调用：${name}(${JSON.stringify(args, null, 2)})` }]);
    } else if (msg.type === 'tool_result') {
      setMessages(m => [...m, { role: 'tool', content: (msg as any).text }]);
    } else if (msg.type === 'step_boundary') {
      currentMsgRef.current = null;
      flushNow();
    } else if (msg.type === 'usage') {
      setUsage((msg as any).usage);
    } else if (msg.type === 'log') {
      // 其他日志不进对话区
    } else if (msg.type === 'done' || msg.type === 'error') {
      setStreaming(false);
      currentMsgRef.current = null;
      flushNow();
      if (msg.type === 'error') {
        setMessages(m => [...m, { role: 'system', content: '错误：' + msg.message }]);
      } else if ((msg as any).interrupted) {
        setMessages(m => [...m, { role: 'system', content: '已中止' }]);
      }
      if ('usage' in msg && msg.usage) setUsage(msg.usage);
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

  const handleSend = (text: string) => {
    setMessages(m => [...m, { role: 'user', content: text }]);
    setStreaming(true);
    setUsage(null);
    wsRef.current?.send(JSON.stringify({ type: 'chat', text }));
  };

  const handleStop = () => wsRef.current?.send(JSON.stringify({ type: 'stop' }));

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
            children={children}
            sessionName={currentSession}
            streaming={streaming}
            onSend={handleSend}
            onStop={handleStop}
          />
        </div>
      </div>
      <StatusBar path={contest?.contest_dir ?? '无比赛工程'} usage={usage} />
    </div>
  );
}
