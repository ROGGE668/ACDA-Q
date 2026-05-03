import { useEffect } from "react";
import { Link } from "react-router-dom";
import { useStrategyStore } from "../stores/strategyStore";

export default function StrategyListPage() {
  const { strategies, loading, loaded, fetchStrategies, removeStrategy } = useStrategyStore();

  useEffect(() => {
    if (!loaded) {
      fetchStrategies();
    }
  }, [loaded, fetchStrategies]);

  const remove = async (id: string) => {
    if (!confirm("确定删除该策略？关联的回测记录也会保留。")) return;
    await removeStrategy(id);
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
      <div style={{ marginTop: "1rem", display: "flex", flexDirection: "column", gap: "0.75rem" }}>
        {strategies.map((s) => (
          <div key={s.id} className="card" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div>
              <h4>{s.name}</h4>
              <p style={{ color: "#94a3b8", fontSize: "0.875rem" }}>{s.description || "暂无描述"}</p>
            </div>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <Link to={`/strategies/${s.id}`}>
                <button className="secondary">编辑</button>
              </Link>
              <button className="secondary" onClick={() => remove(s.id)} style={{ color: "#ef4444", borderColor: "#7f1d1d" }}>
                删除
              </button>
            </div>
          </div>
        ))}
        {strategies.length === 0 && (
          <div className="card" style={{ textAlign: "center", color: "#94a3b8" }}>
            暂无策略，点击右上角新建。
          </div>
        )}
      </div>
    </div>
  );
}
