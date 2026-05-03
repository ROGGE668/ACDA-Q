import { useEffect, useState } from "react";
import { subscriptionAPI, deviceAPI, paymentAPI } from "../services/api";

interface SubscriptionStatus {
  tier: string;
  status: string;
  expires_at: string | null;
  max_devices: number;
  ai_quota_daily: number;
  backtest_quota_daily: number;
  devices_active: number;
  ai_used_today: number;
  backtest_used_today: number;
}

interface Device {
  id: string;
  device_fingerprint: string;
  device_name: string | null;
  os_type: string | null;
  last_heartbeat_at: string | null;
  is_active: boolean;
  revoked_at: string | null;
  created_at: string;
}

export default function SubscriptionPage() {
  const [sub, setSub] = useState<SubscriptionStatus | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedTier, setSelectedTier] = useState<"pro" | "enterprise">("pro");
  const [selectedDuration, setSelectedDuration] = useState(1);
  const [selectedChannel, setSelectedChannel] = useState<"alipay" | "wechat">("alipay");
  const [paying, setPaying] = useState(false);

  const fetchData = async () => {
    setLoading(true);
    try {
      const [subRes, devRes] = await Promise.all([
        subscriptionAPI.status(),
        deviceAPI.list(),
      ]);
      setSub(subRes.data);
      setDevices(devRes.data || []);
    } catch (e) {
      console.error("Failed to load subscription data:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const revokeDevice = async (id: string) => {
    if (!confirm("确定吊销该设备？")) return;
    try {
      await deviceAPI.revoke(id);
      setDevices((prev) => prev.map((d) => (d.id === id ? { ...d, is_active: false, revoked_at: new Date().toISOString() } : d)));
    } catch (e: any) {
      alert(e.response?.data?.detail || "吊销失败");
    }
  };

  const createPayment = async () => {
    setPaying(true);
    try {
      const { data } = await paymentAPI.create({
        channel: selectedChannel,
        tier: selectedTier,
        duration_months: selectedDuration,
      });
      alert(`订单已创建: ${data.order_no}\n请使用 ${selectedChannel === "alipay" ? "支付宝" : "微信"} 扫码支付`);
    } catch (e: any) {
      alert(e.response?.data?.detail || "创建订单失败");
    } finally {
      setPaying(false);
    }
  };

  if (loading) {
    return <div className="card">加载中...</div>;
  }

  const tierNames: Record<string, string> = {
    free: "免费版",
    pro: "专业版",
    enterprise: "企业版",
  };

  return (
    <div>
      <h1>订阅管理</h1>

      {/* 订阅状态 */}
      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>当前套餐</h3>
        {sub ? (
          <div style={{ marginTop: "0.75rem", display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: "1rem" }}>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>套餐</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{tierNames[sub.tier] || sub.tier}</p>
            </div>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>状态</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{sub.status === "active" ? "有效" : sub.status}</p>
            </div>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>到期时间</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{sub.expires_at ? new Date(sub.expires_at).toLocaleDateString() : "永久"}</p>
            </div>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>设备</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{sub.devices_active} / {sub.max_devices}</p>
            </div>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>AI 今日用量</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{sub.ai_used_today} / {sub.ai_quota_daily}</p>
            </div>
            <div>
              <label style={{ fontSize: "0.75rem", color: "var(--muted)" }}>回测今日用量</label>
              <p style={{ fontSize: "1.125rem", fontWeight: 600 }}>{sub.backtest_used_today} / {sub.backtest_quota_daily}</p>
            </div>
          </div>
        ) : (
          <p style={{ color: "var(--muted)" }}>无法加载订阅信息</p>
        )}
      </div>

      {/* 设备管理 */}
      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>设备管理</h3>
        <div style={{ marginTop: "0.75rem", display: "flex", flexDirection: "column", gap: "0.5rem" }}>
          {devices.map((d) => (
            <div key={d.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "0.5rem", border: "1px solid var(--border)", borderRadius: "0.375rem" }}>
              <div>
                <p style={{ fontWeight: 500 }}>{d.device_name || "未命名设备"} <span style={{ fontSize: "0.75rem", color: "var(--muted)" }}>({d.os_type})</span></p>
                <p style={{ fontSize: "0.75rem", color: "var(--muted)" }}>指纹: {d.device_fingerprint.slice(0, 16)}...</p>
                <p style={{ fontSize: "0.75rem", color: d.is_active ? "#22c55e" : "#ef4444" }}>
                  {d.is_active ? "活跃" : "已吊销"} · 最后心跳: {d.last_heartbeat_at ? new Date(d.last_heartbeat_at).toLocaleString() : "无"}
                </p>
              </div>
              {d.is_active && !d.revoked_at && (
                <button className="secondary" onClick={() => revokeDevice(d.id)} style={{ color: "#ef4444", borderColor: "#7f1d1d" }}>
                  吊销
                </button>
              )}
            </div>
          ))}
          {devices.length === 0 && <p style={{ color: "var(--muted)", fontSize: "0.875rem" }}>暂无注册设备</p>}
        </div>
      </div>

      {/* 升级套餐 */}
      <div className="card" style={{ marginTop: "1rem" }}>
        <h3>升级套餐</h3>
        <div style={{ marginTop: "0.75rem", display: "flex", flexDirection: "column", gap: "1rem" }}>
          <div style={{ display: "flex", gap: "0.5rem" }}>
            <button className={selectedTier === "pro" ? "" : "secondary"} onClick={() => setSelectedTier("pro")}>
              专业版 (¥299/月)
            </button>
            <button className={selectedTier === "enterprise" ? "" : "secondary"} onClick={() => setSelectedTier("enterprise")}>
              企业版 (¥999/月)
            </button>
          </div>

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>时长</label>
            <select value={selectedDuration} onChange={(e) => setSelectedDuration(Number(e.target.value))}>
              <option value={1}>1 个月</option>
              <option value={3}>3 个月</option>
              <option value={6}>6 个月</option>
              <option value={12}>12 个月</option>
            </select>
          </div>

          <div>
            <label style={{ fontSize: "0.875rem", color: "var(--muted)", display: "block", marginBottom: "0.25rem" }}>支付方式</label>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <button className={selectedChannel === "alipay" ? "" : "secondary"} onClick={() => setSelectedChannel("alipay")}>
                支付宝
              </button>
              <button className={selectedChannel === "wechat" ? "" : "secondary"} onClick={() => setSelectedChannel("wechat")}>
                微信支付
              </button>
            </div>
          </div>

          <div style={{ fontSize: "1.125rem", fontWeight: 600 }}>
            总计: ¥{((selectedTier === "pro" ? 29900 : 99900) * selectedDuration / 100).toFixed(0)}
          </div>

          <button onClick={createPayment} disabled={paying}>
            {paying ? "处理中..." : "立即支付"}
          </button>
        </div>
      </div>
    </div>
  );
}
