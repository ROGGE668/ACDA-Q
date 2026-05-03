import { Component, ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error?: Error;
}

export default class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error("App crashed:", error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: "100vh",
          flexDirection: "column",
          gap: "1rem",
          padding: "2rem",
          textAlign: "center",
        }}>
          <div style={{ fontSize: "3rem" }}>💥</div>
          <h2>应用出错了</h2>
          <p style={{ color: "var(--muted)", maxWidth: 400 }}>
            抱歉，ACDA-Quant 遇到了意外错误。您可以尝试刷新页面或重启应用。
          </p>
          <pre style={{
            background: "var(--surface)",
            padding: "1rem",
            borderRadius: "0.5rem",
            fontSize: "0.75rem",
            maxWidth: 600,
            overflow: "auto",
            textAlign: "left",
          }}>
            {this.state.error?.toString()}
          </pre>
          <div style={{ display: "flex", gap: "0.5rem" }}>
            <button onClick={() => window.location.reload()}>刷新页面</button>
            <button className="secondary" onClick={() => {
              localStorage.clear();
              window.location.reload();
            }}>
              清除缓存并重载
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
