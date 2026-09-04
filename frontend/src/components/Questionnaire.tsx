import { useState } from 'react';

export interface AskQuestion {
  type: 'single' | 'multi' | 'text';
  question: string;
  options?: string[];
}

export interface AskAnswer {
  type: string;
  question: string;
  answer: string | string[];
}

/** 问卷交互控件：每行一个问题，底部提交/取消。 */
export function Questionnaire({
  questions,
  onSubmit,
  onCancel,
}: {
  questions: AskQuestion[];
  onSubmit: (answers: AskAnswer[]) => void;
  onCancel: () => void;
}) {
  // 每题的选中项（单选存 1 个，多选存多个）
  const [selections, setSelections] = useState<Record<number, string[]>>({});
  // 每题的自由输入（“我来告诉 agent” 或填空）
  const [texts, setTexts] = useState<Record<number, string>>({});

  const toggleOption = (qi: number, opt: string, multi: boolean) => {
    setSelections(prev => {
      const cur = prev[qi] ?? [];
      if (multi) {
        return { ...prev, [qi]: cur.includes(opt) ? cur.filter(x => x !== opt) : [...cur, opt] };
      }
      return { ...prev, [qi]: [opt] };
    });
  };

  const buildAnswers = (): AskAnswer[] => {
    return questions.map((q, qi) => {
      if (q.type === 'text') {
        return { type: 'text', question: q.question, answer: texts[qi] ?? '' };
      }
      const selected = selections[qi] ?? [];
      const custom = (texts[qi] ?? '').trim();
      if (q.type === 'single') {
        return { type: 'single', question: q.question, answer: custom || selected[0] || '' };
      }
      return { type: 'multi', question: q.question, answer: [...selected, ...(custom ? [custom] : [])] };
    });
  };

  return (
    <div className="questionnaire">
      <div className="questionnaire-title">📋 Agent 请求回答</div>
      {questions.map((q, qi) => (
        <div key={qi} className="question-item">
          <div className="question-text">{qi + 1}. {q.question}</div>
          {q.type === 'text' ? (
            <input
              className="question-input"
              type="text"
              value={texts[qi] ?? ''}
              onChange={e => setTexts(prev => ({ ...prev, [qi]: e.target.value }))}
              placeholder="输入回答..."
            />
          ) : (
            <>
              {(q.options ?? []).map(opt => (
                <label key={opt} className="question-option">
                  <input
                    type={q.type === 'single' ? 'radio' : 'checkbox'}
                    name={`q${qi}`}
                    checked={(selections[qi] ?? []).includes(opt)}
                    onChange={() => toggleOption(qi, opt, q.type === 'multi')}
                  />
                  {opt}
                </label>
              ))}
              <label className="question-option">
                <input
                  type={q.type === 'single' ? 'radio' : 'checkbox'}
                  name={`q${qi}`}
                  checked={texts[qi] !== undefined && texts[qi] !== ''}
                  onChange={() => setTexts(prev => ({ ...prev, [qi]: prev[qi] ? '' : ' ' }))}
                />
                我来告诉 agent：
              </label>
              {(texts[qi] ?? '') !== '' && (
                <input
                  className="question-input"
                  type="text"
                  autoFocus
                  value={texts[qi]}
                  onChange={e => setTexts(prev => ({ ...prev, [qi]: e.target.value }))}
                  placeholder="自行输入回答..."
                />
              )}
            </>
          )}
        </div>
      ))}
      <div className="questionnaire-actions">
        <button className="btn-send" onClick={() => onSubmit(buildAnswers())}>提交</button>
        <button className="btn" onClick={onCancel}>取消</button>
      </div>
    </div>
  );
}
