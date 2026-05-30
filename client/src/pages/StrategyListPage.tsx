import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Link } from "react-router-dom";
import { useStrategyStore } from "../stores/strategyStore";

export default function StrategyListPage() {
  const { strategies, loading, loaded, fetchStrategies, removeStrategy, saveStrategy } = useStrategyStore();

  useEffect(() => {
    if (!loaded) {
      fetchStrategies();
    }
  }, [loaded, fetchStrategies]);

  const navigate = useNavigate();
  const [removeError, setRemoveError] = useState("");
  const [duplicating, setDuplicating] = useState<string | null>(null);

  const duplicate = async (s: { id: string; name: string; description?: string; code: string; params?: Record<string, any>; strategy_type?: string }) => {
    try {
      setDuplicating(s.id);
      const newName = s.name + " (副本)";
      const saved = await saveStrategy({
        name: newName,
        description: s.description || "",
        code: s.code,
        type: s.strategy_type || "single_stock",
        params: s.params || {},
      });
      navigate(`/strategies/${saved.id}`);
    } catch (e: any) {
      setRemoveError(e?.response?.data?.error || e?.message || "复制失败");
    } finally {
      setDuplicating(null);
    }
  };
  const remove = async (id: string) => {
    try {
      setRemoveError("");
      await removeStrategy(id);
    } catch (e: any) {
      const msg = e?.response?.data?.error || e?.message || "删除失败";
      setRemoveError(msg);
      console.error("[StrategyList] Delete failed:", e);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1>我的策略</h1>
        <div style={{ display: "flex", gap: "0.5rem" }}>
          <button className="secondary" onClick={fetchStrategies} disabled={loading}>
            {loading ? "刷新中..." : "刷新"}
          </button>
          <Link to="/strategies/new">
            <button>新建策略</button>
          </Link>
        </div>
      </div>
      {removeError && (
        <div style={{ marginTop: "0.5rem", padding: "0.5rem", background: "#7f1d1d", color: "#fca5a5", borderRadius: "0.375rem", fontSize: "0.875rem" }}>
          {removeError}
        </div>
      )}
      <div style={{ marginTop: "1rem", display: "flex", flexDirection: "column", gap: "0.75rem" }}>
        {strategies.map((s) => (
          <div key={s.id} className="card" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div>
              <h4>{s.name}</h4>
              <p style={{ color: "var(--muted)", fontSize: "0.875rem" }}>{s.description || "暂无描述"}</p>
            </div>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <Link to={`/strategies/${s.id}`}>
                <button className="secondary">编辑</button>
              </Link>
              <button className="secondary" onClick={() => duplicate(s)} disabled={duplicating === s.id}>
                {duplicating === s.id ? "复制中..." : "复制"}
              </button>
              <button className="secondary" onClick={() => remove(s.id)} style={{ color: "#ef4444", borderColor: "#7f1d1d" }}>
                删除
              </button>
            </div>
          </div>
        ))}
        {strategies.length === 0 && (
          <div className="card" style={{ textAlign: "center", color: "var(--muted)" }}>
            暂无策略，点击右上角新建。
          </div>
        )}
      </div>
    </div>
  );
}
