export default function ProductInfoPage() {
  return (
    <div style={{ maxWidth: 800, margin: "0 auto", padding: "2rem 1rem" }}>
      <h1>产品说明</h1>

      <div className="card" style={{ marginTop: "1.5rem" }}>
        <h3>ACDA-Quant 是什么？</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          ACDA-Quant 是一款面向 A 股市场的桌面量化投资回测平台。
          它结合了本地策略开发、AI 辅助代码生成和云端回测执行能力，
          为个人投资者和量化研究员提供从策略构思到历史验证的完整工作流。
        </p>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>核心功能</h3>
        <ul style={{ marginLeft: "1.5rem", marginTop: "0.5rem", lineHeight: 1.8 }}>
          <li><strong>本地策略开发</strong>：策略代码仅保存在本地，不上传服务器，保护您的知识产权</li>
          <li><strong>AI 辅助生成</strong>：通过自然语言描述，由 AI 生成可执行的 Python 策略代码</li>
          <li><strong>多模式回测</strong>：支持个股回测、组合回测和全市场扫描三种模式</li>
          <li><strong>沙箱执行</strong>：回测任务在隔离子进程中运行，限制资源使用，确保安全</li>
          <li><strong>实时推送</strong>：通过 WebSocket 实时获取回测进度和结果</li>
          <li><strong>订阅管理</strong>：灵活的订阅套餐，按需选择功能配额</li>
        </ul>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>订阅套餐</h3>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))", gap: "1rem", marginTop: "0.75rem" }}>
          <div style={{ padding: "1rem", border: "1px solid var(--border)", borderRadius: "0.5rem" }}>
            <h4 style={{ margin: "0 0 0.5rem 0" }}>免费版</h4>
            <p style={{ fontSize: "0.875rem", color: "var(--muted)" }}>¥0 / 月</p>
            <ul style={{ marginTop: "0.5rem", fontSize: "0.875rem", lineHeight: 1.8 }}>
              <li>1 台设备</li>
              <li>每日 5 次 AI 生成</li>
              <li>每日 10 次回测</li>
              <li>基础市场数据</li>
            </ul>
          </div>
          <div style={{ padding: "1rem", border: "1px solid var(--border)", borderRadius: "0.5rem" }}>
            <h4 style={{ margin: "0 0 0.5rem 0" }}>专业版</h4>
            <p style={{ fontSize: "0.875rem", color: "var(--muted)" }}>¥299 / 月</p>
            <ul style={{ marginTop: "0.5rem", fontSize: "0.875rem", lineHeight: 1.8 }}>
              <li>3 台设备</li>
              <li>每日 50 次 AI 生成</li>
              <li>每日 100 次回测</li>
              <li>完整市场数据</li>
            </ul>
          </div>
          <div style={{ padding: "1rem", border: "1px solid var(--border)", borderRadius: "0.5rem" }}>
            <h4 style={{ margin: "0 0 0.5rem 0" }}>企业版</h4>
            <p style={{ fontSize: "0.875rem", color: "var(--muted)" }}>¥999 / 月</p>
            <ul style={{ marginTop: "0.5rem", fontSize: "0.875rem", lineHeight: 1.8 }}>
              <li>10 台设备</li>
              <li>无限 AI 生成</li>
              <li>无限回测</li>
              <li>优先技术支持</li>
            </ul>
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>技术架构</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          客户端基于 Tauri 2.0 + React 构建，提供原生桌面体验；
          服务端采用 FastAPI + SQLAlchemy 2.0 + Celery + Redis + TimescaleDB，
          支持高并发回测任务调度和时序数据查询。
        </p>
      </div>
    </div>
  );
}
