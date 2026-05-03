import { Outlet, Link, useNavigate, useLocation } from "react-router-dom";
import { useEffect } from "react";
import { useAuthStore } from "../stores/authStore";

export default function Layout() {
  const { user, fetchMe, logout } = useAuthStore();
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    fetchMe().catch(() => navigate("/login"));
  }, [fetchMe, navigate]);

  const navLink = (to: string, label: string) => {
    const active = location.pathname === to || location.pathname.startsWith(to + "/");
    return (
      <Link
        to={to}
        style={{
          padding: "0.5rem 0.75rem",
          borderRadius: "0.375rem",
          background: active ? "#334155" : "transparent",
          color: active ? "#38bdf8" : "#e2e8f0",
          fontWeight: active ? 600 : 400,
        }}
      >
        {label}
      </Link>
    );
  };

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <aside style={{ width: 220, background: "#1e293b", borderRight: "1px solid #334155", padding: "1rem", display: "flex", flexDirection: "column" }}>
        <div style={{ marginBottom: "1.5rem", display: "flex", alignItems: "center", gap: "0.5rem" }}>
          <div
            style={{
              width: 36,
              height: 36,
              borderRadius: "0.5rem",
              background: "linear-gradient(135deg, #38bdf8, #0ea5e9)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "#0f172a",
              fontWeight: 800,
              fontSize: "0.875rem",
              letterSpacing: "0.05em",
              flexShrink: 0,
            }}
          >
            AQ
          </div>
          <span style={{ fontSize: "1.125rem", fontWeight: 700, color: "#e2e8f0", letterSpacing: "0.02em" }}>ACDA-Quant</span>
        </div>
        <nav style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
          {navLink("/", "仪表盘")}
          {navLink("/strategies", "策略中心")}
          {navLink("/strategy-backtest", "策略回测")}
          {navLink("/backtests", "回测记录")}
          {navLink("/subscription", "订阅管理")}
          {navLink("/settings", "设置")}
        </nav>
        <div style={{ marginTop: "auto", paddingTop: "2rem" }}>
          <div style={{ color: "#94a3b8", fontSize: "0.75rem" }}>
            {user?.email}
          </div>
          <button className="secondary" style={{ marginTop: "0.5rem", width: "100%" }} onClick={() => { logout(); navigate("/login"); }}>
            退出登录
          </button>
        </div>
      </aside>
      <main style={{ flex: 1, padding: "1.5rem", overflow: "auto" }}>
        <Outlet />
      </main>
    </div>
  );
}
