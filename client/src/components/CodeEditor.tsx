import { useEffect, useRef, useState } from "react";

interface CodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  maxLength?: number;
}

export default function CodeEditor({ value, onChange, rows = 16, maxLength }: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [highlighted, setHighlighted] = useState(value);

  // 轻量级语法高亮（不依赖prismjs时也能工作）
  useEffect(() => {
    let html = value
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");

    // 关键词高亮
    const keywords = [
      "class", "def", "return", "if", "elif", "else", "for", "in", "while",
      "import", "from", "as", "try", "except", "pass", "self", "and", "or", "not",
      "True", "False", "None",
    ];
    keywords.forEach((kw) => {
      const re = new RegExp(`\\b${kw}\\b`, "g");
      html = html.replace(re, `<span style="color:#c678dd">${kw}</span>`);
    });

    // 字符串高亮
    html = html.replace(
      /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g,
      '<span style="color:#98c379">$1</span>'
    );

    // 数字高亮
    html = html.replace(
      /\b(\d+(?:\.\d+)?)\b/g,
      '<span style="color:#d19a66">$1</span>'
    );

    // 注释高亮
    html = html.replace(
      /(#.*$)/gm,
      '<span style="color:#5c6370">$1</span>'
    );

    setHighlighted(html + "\n");
  }, [value]);

  const handleScroll = () => {
    if (textareaRef.current && preRef.current) {
      preRef.current.scrollTop = textareaRef.current.scrollTop;
      preRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  };

  return (
    <div style={{ position: "relative", fontFamily: "monospace", fontSize: "0.875rem" }}>
      <pre
        ref={preRef}
        aria-hidden="true"
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          margin: 0,
          padding: "0.5rem",
          background: "#1e1e1e",
          color: "#abb2bf",
          borderRadius: "0.375rem",
          overflow: "auto",
          whiteSpace: "pre",
          wordWrap: "normal",
          pointerEvents: "none",
          zIndex: 1,
        }}
        dangerouslySetInnerHTML={{ __html: highlighted }}
      />
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onScroll={handleScroll}
        rows={rows}
        maxLength={maxLength}
        spellCheck={false}
        style={{
          position: "relative",
          zIndex: 2,
          width: "100%",
          padding: "0.5rem",
          background: "transparent",
          color: "transparent",
          caretColor: "#fff",
          border: "1px solid var(--border)",
          borderRadius: "0.375rem",
          fontFamily: "inherit",
          fontSize: "inherit",
          lineHeight: "1.5",
          resize: "vertical",
          whiteSpace: "pre",
          wordWrap: "normal",
        }}
      />
    </div>
  );
}
