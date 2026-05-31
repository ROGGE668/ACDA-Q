import { Component, ReactNode } from "react";

interface Props {
  children: ReactNode;
  title?: string;
}

interface State {
  hasError: boolean;
  error?: Error;
}

export default class SectionErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error(`[Section ${this.props.title || "unknown"}] crashed:`, error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="card" style={{ marginTop: "1rem", padding: "1rem", textAlign: "center" }}>
          <p style={{ color: "#ef4444", fontSize: "0.875rem" }}>
            {this.props.title ? `${this.props.title} 加载失败` : "模块加载失败"}
          </p>
          <p style={{ color: "var(--muted)", fontSize: "0.75rem", marginTop: "0.25rem" }}>
            {this.state.error?.message || "未知错误"}
          </p>
        </div>
      );
    }
    return this.props.children;
  }
}
