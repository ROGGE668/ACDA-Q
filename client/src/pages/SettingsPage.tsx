import { useEffect, useState } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import { useAISettingsStore } from "../stores/aiSettingsStore";

export default function SettingsPage() {
  const { apiBase, theme, globalShortcutEnabled, loaded, loadSettings, setApiBase, setTheme, setGlobalShortcutEnabled } = useSettingsStore();
  const { deepseekApiKey, deepseekBaseUrl, deepseekModel, loaded: aiLoaded, loadSettings: loadAISettings, setDeepseekApiKey, setDeepseekBaseUrl, setDeepseekModel } = useAISettingsStore();

  const [localApiBase, setLocalApiBase] = useState(apiBase);
  const [localApiKey, setLocalApiKey] = useState(deepseekApiKey);
  const [localBaseUrl, setLocalBaseUrl] = useState(deepseekBaseUrl);
  const [localModel, setLocalModel] = useState(deepseekModel);
  const [saved, setSaved] = useState(false);
  const [aiSaved, setAiSaved] = useState(false);

  useEffect(() => {
    if (!loaded) loadSettings();
    if (!aiLoaded) loadAISettings();
  }, [loaded, loadSettings, aiLoaded, loadAISettings]);

  useEffect(() => {
    setLocalApiBase(apiBase);
  }, [apiBase]);

  useEffect(() => {
    setLocalApiKey(deepseekApiKey);
    setLocalBaseUrl(deepseekBaseUrl);
    setLocalModel(deepseekModel);
  }, [deepseekApiKey, deepseekBaseUrl, deepseekModel]);

  const saveApiBase = async () => {
    await setApiBase(localApiBase);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const saveAISettings = async () => {
    await setDeepseekApiKey(localApiKey);
    await setDeepseekBaseUrl(localBaseUrl);
    await setDeepseekModel(localModel);
    setAiSaved(true);
    setTimeout(() => setAiSaved(false), 2000);
  };

  if (!loaded || !aiLoaded) {
    return <div className="card">加载中...</div>;
  }

  return (
    <div>
      <h1>设置</h1>

      <div className="card" style={{ marginTop: "1rem", maxWidth: 600 }}>
        <h3>通用</h3>
        <div style={{ display: "flex", flexDirection: "column", gap: "1rem", marginTop: "1rem" }}>

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              API 服务器地址
            </label>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <input
                value={localApiBase}
                onChange={(e) => setLocalApiBase(e.target.value)}
                placeholder="http://localhost:8000/api/v1"
              />
              <button onClick={saveApiBase}>保存</button>
            </div>
            {saved && <span style={{ color: "var(--success)", fontSize: "0.75rem" }}>已保存</span>}
            <p style={{ fontSize: "0.75rem", color: "var(--muted)", marginTop: "0.25rem" }}>
              修改后需刷新应用生效
            </p>
          </div>

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

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              全局快捷键
            </label>
            <div style={{ display: "flex", alignItems: "center", gap: "0.75rem" }}>
              <input
                type="checkbox"
                checked={globalShortcutEnabled}
                onChange={(e) => setGlobalShortcutEnabled(e.target.checked)}
                style={{ width: "auto" }}
              />
              <span style={{ fontSize: "0.875rem" }}>
                启用 Cmd+Shift+S 显示/隐藏窗口
              </span>
            </div>
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: "1rem", maxWidth: 600 }}>
        <h3>AI 配置</h3>
        <div style={{ display: "flex", flexDirection: "column", gap: "1rem", marginTop: "1rem" }}>
          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              DeepSeek API Key
            </label>
            <input
              type="password"
              value={localApiKey}
              onChange={(e) => setLocalApiKey(e.target.value)}
              placeholder="sk-..."
            />
            <p style={{ fontSize: "0.75rem", color: "var(--muted)", marginTop: "0.25rem" }}>
              您的 API Key 仅保存在本地，不会上传到服务器
            </p>
          </div>

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              API 基础地址
            </label>
            <input
              value={localBaseUrl}
              onChange={(e) => setLocalBaseUrl(e.target.value)}
              placeholder="https://api.deepseek.com"
            />
          </div>

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>
              模型
            </label>
            <input
              value={localModel}
              onChange={(e) => setLocalModel(e.target.value)}
              placeholder="deepseek-chat"
            />
          </div>

          <div>
            <button onClick={saveAISettings}>保存 AI 配置</button>
            {aiSaved && <span style={{ color: "var(--success)", fontSize: "0.75rem", marginLeft: "0.5rem" }}>已保存</span>}
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: "1rem", maxWidth: 600 }}>
        <h3>关于</h3>
        <div style={{ marginTop: "0.75rem", color: "var(--muted)", fontSize: "0.875rem" }}>
          <p>ACDA-Quant v0.2.0</p>
          <p style={{ marginTop: "0.5rem" }}>A股量化投资平台</p>
          <p style={{ marginTop: "0.5rem" }}>快捷键：</p>
          <ul style={{ marginLeft: "1.25rem", marginTop: "0.25rem" }}>
            <li>Cmd+Shift+S — 显示/隐藏窗口</li>
            <li>Cmd+, — 打开设置</li>
            <li>Cmd+R — 重新加载</li>
          </ul>
        </div>
      </div>
    </div>
  );
}
