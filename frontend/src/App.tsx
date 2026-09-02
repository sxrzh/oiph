import { useEffect, useRef, useState, useCallback } from 'react';
import { getContest, getProblem, getSessions } from './api';
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

export default function App() {
  const [contest, setContest] = useState<ContestData | null>(null);
  const [currentProblemId, setCurrentProblemId] = useState<string | null>(null);
  const [problem, setProblem] = useState<ProblemDetail | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [currentSession, setCurrentSession] = useState<string | null>(null);
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [usage, setUsage] = useState<any>(null);
  const [leftWidth, setLeftWidth] = useState(45); // percentage
  const wsRef = useRef<WebSocket | null>(null);
  const currentMsgRef = useRef<DisplayMessage | null>(null);

  // Load contest
  const loadContest = useCallback(async () => {
    const d = await getContest();
    setContest(d);
    if (d.problems.length > 0 && !currentProblemId) {
      setCurrentProblemId(d.problems[0].id);
    }
  }, [currentProblemId]);

  // Load problem
  const loadProblem = useCallback(async () => {
    if (!currentProblemId) return;
    const d: any = await getProblem(currentProblemId);
    if (!d.error) setProblem(d);
  }, [currentProblemId]);

  // Load sessions
  const loadSessions = useCallback(async () => {
    const d = await getSessions();
    setSessions(d.sessions);
    setCurrentSession(d.current);
  }, []);

  useEffect(() => { loadContest(); }, [loadContest]);
  useEffect(() => { loadProblem(); }, [loadProblem]);
  useEffect(() => { loadSessions(); }, [loadSessions]);
  useEffect(() => {
    const poll = setInterval(() => { loadContest(); loadSessions(); }, 5000);
    return () => clearInterval(poll);
  }, [loadContest, loadSessions]);

  // WebSocket
  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    wsRef.current = ws;

    ws.onmessage = (e) => {
      const msg: WsMessage = JSON.parse(e.data);
      handleWsMessage(msg);
    };
    ws.onclose = () => {
      setTimeout(() => location.reload(), 2000);
    };
    return () => ws.close();
  }, []);

  const handleWsMessage = (msg: WsMessage) => {
    if (msg.type === 'content') {
      if (!currentMsgRef.current || currentMsgRef.current.role !== 'assistant') {
        currentMsgRef.current = { role: 'assistant', content: '' };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      setMessages(m => [...m]); // trigger re-render
    } else if (msg.type === 'reasoning') {
      if (!currentMsgRef.current || currentMsgRef.current.role !== 'reasoning') {
        currentMsgRef.current = { role: 'reasoning', content: '' };
        setMessages(m => [...m, currentMsgRef.current!]);
      }
      currentMsgRef.current.content += msg.text;
      setMessages(m => [...m]);
    } else if (msg.type === 'log') {
      if (msg.text.startsWith('工具调用：') || msg.text.startsWith('->')) {
        setMessages(m => [...m, { role: 'tool', content: msg.text }]);
      }
    } else if (msg.type === 'done' || msg.type === 'error') {
      setStreaming(false);
      currentMsgRef.current = null;
      if (msg.type === 'error') {
        setMessages(m => [...m, { role: 'system', content: '错误：' + msg.message }]);
      }
      if ('usage' in msg && msg.usage) {
        setUsage(msg.usage);
      }
    } else if (msg.type === 'messages') {
      // Full message list refresh
      const displayMsgs: DisplayMessage[] = msg.messages
        .filter((m: any) => m.role !== 'system')
        .map((m: any) => ({
          role: m.role,
          content: m.content || '',
          toolCalls: m.tool_calls?.map((tc: any) => `[${tc.function.name}(${tc.function.arguments})]`).join('\n'),
        }));
      setMessages(displayMsgs);
    }
  };

  const handleSend = (text: string) => {
    setMessages(m => [...m, { role: 'user', content: text }]);
    setStreaming(true);
    setUsage(null);
    wsRef.current?.send(JSON.stringify({ type: 'chat', text }));
  };

  const handleStop = () => {
    wsRef.current?.send(JSON.stringify({ type: 'stop' }));
  };

  // Resizable splitter
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
              <div
                key={p.id}
                className={`problem-tab ${p.id === currentProblemId ? 'active' : ''}`}
                onClick={() => setCurrentProblemId(p.id)}
              >
                {p.name || p.id}
              </div>
            ))}
          </div>
          <ProblemArea problem={problem} onRefresh={loadProblem} />
        </div>
        <div className="splitter" onMouseDown={handleSplitterMouseDown} />
        <div className="right-panel">
          <SessionBar sessions={sessions} current={currentSession} onSwitched={loadSessions} />
          <ChatArea
            messages={messages}
            streaming={streaming}
            onSend={handleSend}
            onStop={handleStop}
          />
        </div>
      </div>
      <StatusBar
        path={contest?.contest_dir ?? '无比赛工程'}
        usage={usage}
      />
    </div>
  );
}
