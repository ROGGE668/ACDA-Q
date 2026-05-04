export function SkeletonCard() {
  return (
    <div className="card" style={{ opacity: 0.5 }}>
      <div style={{ height: "1.25rem", width: "40%", background: "var(--border)", borderRadius: "0.25rem", marginBottom: "0.5rem" }} />
      <div style={{ height: "1rem", width: "70%", background: "var(--border)", borderRadius: "0.25rem" }} />
    </div>
  );
}

export function SkeletonTable({ rows = 5 }: { rows?: number }) {
  return (
    <div className="card">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} style={{ display: "flex", gap: "0.5rem", marginBottom: "0.5rem", opacity: 0.5 }}>
          <div style={{ height: "1rem", flex: 2, background: "var(--border)", borderRadius: "0.25rem" }} />
          <div style={{ height: "1rem", flex: 1, background: "var(--border)", borderRadius: "0.25rem" }} />
          <div style={{ height: "1rem", flex: 1, background: "var(--border)", borderRadius: "0.25rem" }} />
        </div>
      ))}
    </div>
  );
}

export function SkeletonMetrics() {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "1rem", opacity: 0.5 }}>
      {Array.from({ length: 4 }).map((_, i) => (
        <div key={i} className="card">
          <div style={{ height: "0.75rem", width: "50%", background: "var(--border)", borderRadius: "0.25rem", marginBottom: "0.5rem" }} />
          <div style={{ height: "1.5rem", width: "30%", background: "var(--border)", borderRadius: "0.25rem" }} />
        </div>
      ))}
    </div>
  );
}
