import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { authAPI } from "../services/api";
import { setTokens } from "../stores/tokenStore";
import { useAuthStore } from "../stores/authStore";

export default function LoginPage() {
  const [isLogin, setIsLogin] = useState(true);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [nickname, setNickname] = useState("");
  const [error, setError] = useState("");
  const navigate = useNavigate();

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    try {
      const res = await (isLogin
        ? authAPI.login(email, password)
        : authAPI.register(email, password, nickname || undefined));
      // 保存 Bearer Token
      if (res.data?.access_token && res.data?.refresh_token) {
        await setTokens(res.data.access_token, res.data.refresh_token);
        // 获取用户信息并更新状态
        await useAuthStore.getState().fetchMe();
      }
      navigate("/");
    } catch (err: any) {
      console.error("Login/Register error:", err);
      const detail = err?.response?.data?.detail || err?.message || "请求失败";
      setError(detail);
    }
  };

  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100vh" }}>
      <div className="card" style={{ width: 360 }}>
        <h2 style={{ textAlign: "center", marginBottom: "1rem" }}>{isLogin ? "登录" : "注册"}</h2>
        <form onSubmit={submit} style={{ display: "flex", flexDirection: "column", gap: "0.75rem" }}>
          <input placeholder="邮箱" type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
          <input placeholder="密码" type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          {!isLogin && <input placeholder="昵称（可选）" value={nickname} onChange={(e) => setNickname(e.target.value)} />}
          {error && <div style={{ color: "#ef4444", fontSize: "0.875rem" }}>{error}</div>}
          <button type="submit">{isLogin ? "登录" : "注册"}</button>
        </form>
        <div style={{ textAlign: "center", marginTop: "1rem", fontSize: "0.875rem", color: "#94a3b8" }}>
          {isLogin ? "还没有账号？" : "已有账号？"}
          <a href="#" onClick={(e) => { e.preventDefault(); setIsLogin(!isLogin); }}>
            {isLogin ? "立即注册" : "立即登录"}
          </a>
        </div>
      </div>
    </div>
  );
}
