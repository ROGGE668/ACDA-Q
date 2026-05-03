export default function PrivacyPolicyPage() {
  return (
    <div style={{ maxWidth: 800, margin: "0 auto", padding: "2rem 1rem" }}>
      <h1>隐私政策</h1>
      <p style={{ color: "var(--muted)", marginTop: "0.5rem" }}>最后更新日期：2026年5月3日</p>

      <div className="card" style={{ marginTop: "1.5rem" }}>
        <h3>1. 信息收集</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          ACDA-Quant 仅收集为您提供服务所必需的最少信息。我们收集的信息包括：
        </p>
        <ul style={{ marginLeft: "1.5rem", marginTop: "0.5rem", lineHeight: 1.8 }}>
          <li><strong>账户信息</strong>：邮箱地址（用于身份验证和账户恢复）</li>
          <li><strong>设备信息</strong>：设备指纹（基于CPU、内存、操作系统信息生成的哈希值，用于订阅管理和防止账号共享）</li>
          <li><strong>使用数据</strong>：回测任务执行记录、AI生成次数（用于配额管理）</li>
        </ul>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>2. 信息存储与安全</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          您的<strong>策略代码仅保存在本地设备</strong>，不会上传到我们的服务器。
          您的 DeepSeek API Key 仅保存在本地 Tauri Store 中，我们不会访问或存储您的 API Key。
          账户密码使用 bcrypt 算法哈希后存储，我们无法还原您的原始密码。
        </p>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>3. 设备指纹</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          为执行订阅管理，我们在首次安装时基于您的硬件和系统信息生成固定设备指纹。
          该指纹为不可逆哈希值，无法反推出原始硬件信息。更换主要硬件将导致指纹变化，
          您可能需要重新激活设备或联系客服。
        </p>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>4. 信息共享</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          我们不会将您的个人信息出售、交易或以其他方式转让给第三方。
          仅在法律要求或保护我们的权利时，我们可能会披露您的信息。
        </p>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>5. 数据保留</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          回测结果报告保留7天，之后自动清理。账户信息在您删除账户后30天内清除。
          本地存储的策略代码和设置完全由您控制，卸载应用即删除。
        </p>
      </div>

      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>6. 联系我们</h3>
        <p style={{ marginTop: "0.75rem", lineHeight: 1.7 }}>
          如有关于隐私政策的疑问，请通过应用内反馈渠道联系我们。
        </p>
      </div>
    </div>
  );
}
