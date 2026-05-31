import { useEffect, useRef } from "react";
import { Navigate } from "react-router-dom";
import { useAuthStore } from "../stores/authStore";

function hasStoredToken(): boolean {
  try {
    const raw = localStorage.getItem("acda_auth_access_token");
    if (!raw) return false;
    const parsed = JSON.parse(raw);
    return typeof parsed === "string" && parsed.length > 0;
  } catch {
    return false;
  }
}

export default function PrivateRoute({ children }: { children: React.ReactNode }) {
  const { user, isLoading, init } = useAuthStore();
  const initCalledRef = useRef(false);

  // Zustand persist 反序列化完成前 user 可能为 null，
  // 但只要 localStorage 中有 token，就不应重定向到登录页。
  // 此处额外尝试调用 init() 恢复用户会话。
  useEffect(() => {
    if (!user && !initCalledRef.current) {
      initCalledRef.current = true;
      init();
    }
  }, [user, init]);

  if (isLoading) {
    return (
      <div style={{ display: "flex", justifyContent: "center", alignItems: "center", height: "100vh" }}>
        <div>加载中...</div>
      </div>
    );
  }

  // 有 token 但 user 还在加载 → 等待
  if (!user && hasStoredToken()) {
    return (
      <div style={{ display: "flex", justifyContent: "center", alignItems: "center", height: "100vh" }}>
        <div>加载中...</div>
      </div>
    );
  }

  // init 尚未完成时短暂等待（避免闪跳登录页）
  if (!user && !initCalledRef.current) {
    return (
      <div style={{ display: "flex", justifyContent: "center", alignItems: "center", height: "100vh" }}>
        <div>加载中...</div>
      </div>
    );
  }

  if (!user) {
    return <Navigate to="/login" replace />;
  }

  return <>{children}</>;
}
