import CodeMirror from '@uiw/react-codemirror';
import { oneDark } from '@codemirror/theme-one-dark';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { cpp } from '@codemirror/lang-cpp';
import { languages } from '@codemirror/language-data';
import { EditorView } from '@codemirror/view';

export type EditorLanguage = 'markdown' | 'cpp';

export function CodeEditor({
  value,
  onChange,
  language,
}: {
  value: string;
  onChange: (v: string) => void;
  language: EditorLanguage;
}) {
  const extensions =
    language === 'markdown'
      ? [markdown({ base: markdownLanguage, codeLanguages: languages })]
      : [cpp()];
  return (
    <div style={{ flex: 1, minHeight: 0, overflow: 'auto', border: '1px solid #333', borderRadius: 4 }}>
      <CodeMirror
        value={value}
        height="100%"
        style={{ height: '100%' }}
        theme={oneDark}
        extensions={[
          ...extensions,
          EditorView.lineWrapping,
        ]}
        onChange={onChange}
        basicSetup={{
          foldGutter: true,
          highlightActiveLine: true,
          autocompletion: true,
        }}
      />
    </div>
  );
}
