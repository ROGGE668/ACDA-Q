import { useEffect, useState } from "react";
import { useSettingsStore } from "../stores/settingsStore";

export default function SettingsPage() {
  const { theme, loaded, loadSettings, setTheme } = useSettingsStore();
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!loaded && !error) {
      loadSettings().catch(() => setError(true));
    }
  }, [loaded, loadSettings, error]);

  if (!loaded) {
    return (
      <div className="card" style={{ textAlign: "center", padding: "2rem" }}>
        <div>加载中...</div>
        {error && (
          <div style={{ marginTop: "1rem" }}>
            <div style={{ color: "var(--muted)", marginBottom: "0.5rem" }}>加载失败</div>
            <button onClick={() => { setError(false); loadSettings().catch(() => setError(true)); }}>
              重试
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div>
      <h1>设置</h1>
      <div className="card" style={{ marginTop: "1rem", maxWidth: 600 }}>
        <h3>通用</h3>
        <div style={{ display: "flex", flexDirection: "column", gap: "1rem", marginTop: "1rem" }}>
          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              主题
            </label>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <button
                className={theme === "dark" ? "" : "secondary"}
                onClick={() => setTheme("dark")}
              >
                🌙 深色
              </button>
              <button
                className={theme === "light" ? "" : "secondary"}
                onClick={() => setTheme("light")}
              >
                ☀️ 浅色
              </button>
            </div>
          </div>


        </div>
      </div>

      <div className="card" style={{ marginTop: "1rem", maxWidth: 600 }}>
        <h3>关于</h3>
        <div style={{ marginTop: "0.75rem", color: "var(--muted)", fontSize: "0.875rem" }}>
          <p>ACDA-Quant v{__APP_VERSION__}</p>
          <p style={{ marginTop: "0.5rem" }}>A股量化投资平台</p>
          <p style={{ marginTop: "0.5rem" }}>快捷键：</p>
          <ul style={{ marginLeft: "1.25rem", marginTop: "0.25rem" }}>
            <li>Cmd+, — 打开设置</li>
            <li>Cmd+R — 重新加载</li>
          </ul>
        </div>
      </div>
    </div>
  );
}
