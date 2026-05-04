import { useEffect, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { backtestAPI } from "../services/api";
import { generateStrategy, extractParamsFromCode } from "../services/aiGenerate";
import { useStrategyStore } from "../stores/strategyStore";
import StockSelector from "../components/StockSelector";
import CodeEditor from "../components/CodeEditor";

const MAX_CODE_LENGTH = 8000;

interface ParamDef {
  name: string;
  default: any;
  type: string;
}

export default function StrategyEditorPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { getStrategy, saveStrategy } = useStrategyStore();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [code, setCode] = useState(`class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        # 示例：遍历每个 symbol，当收盘价大于开盘价时买入10%
        for symbol in bar_group["symbol"].unique():
            bar = bar_group[bar_group["symbol"] == symbol]
            if bar.empty:
                continue
            close = float(bar["close"].values[0])
            open_price = float(bar["open"].values[0])
            if close > open_price and context.positions.get(symbol, 0) == 0:
                context.buy(symbol, percent=0.1)
            elif close < open_price and context.positions.get(symbol, 0) > 0:
                context.sell(symbol, percent=1.0)
`);
  const [prompt, setPrompt] = useState("");
  const [generating, setGenerating] = useState(false);

  // 验证
  const [validStatus, setValidStatus] = useState<"idle" | "valid" | "invalid">("idle");
  const [validMsg, setValidMsg] = useState("");

  // 策略参数面板
  const [params, setParams] = useState<ParamDef[]>([]);

  // 回测参数
  const [startDate, setStartDate] = useState("2024-01-01");
  const [endDate, setEndDate] = useState("2024-06-01");
  const [initialCash, setInitialCash] = useState(1000000);
  const [backtesting, setBacktesting] = useState(false);
  const [scope, setScope] = useState<"single" | "portfolio" | "scan">("single");
  const [weightMode, setWeightMode] = useState("equal");
  const [rebalanceFreq, setRebalanceFreq] = useState("1M");

  const [selectedStocks, setSelectedStocks] = useState<string[]>([]);
  const [fullMarketScan, setFullMarketScan] = useState(false);

  useEffect(() => {
    if (id) {
      const s = getStrategy(id);
      if (s) {
        setName(s.name);
        setDescription(s.description || "");
        if (s.code) setCode(s.code);
      }
    }
  }, [id, getStrategy]);

  // 代码变更后防抖提取参数（500ms）
  useEffect(() => {
    if (!code.trim()) {
      setParams([]);
      return;
    }
    const timer = setTimeout(() => {
      // 本地正则提取参数
      extractParamsFromCode(code)
        .then((res) => {
          if (res.params.length > 0) {
            setParams(res.params);
          } else {
            extractParamsLocal();
          }
        })
        .catch(() => extractParamsLocal());
    }, 500);
    return () => clearTimeout(timer);
  }, [code]);

  const extractParamsLocal = () => {
    // 本地正则提取作为 fallback
    const regex = /self\.params\.get\(["'](\w+)["']\s*,\s*([^)]+)\)/g;
    const found: ParamDef[] = [];
    let match;
    while ((match = regex.exec(code)) !== null) {
      const name = match[1];
      const raw = match[2].trim();
      let defVal: any = raw;
      let type = "str";
      try {
        defVal = JSON.parse(raw);
        type = typeof defVal;
        if (type === "number" && Number.isInteger(defVal)) type = "int";
      } catch {
        if (raw === "True" || raw === "False") {
          defVal = raw === "True";
          type = "bool";
        }
      }
      if (!found.find((p) => p.name === name)) {
        found.push({ name, default: defVal, type });
      }
    }
    setParams(found);
  };

  const codeOverLimit = code.length > MAX_CODE_LENGTH;

  const save = async () => {
    if (!name.trim()) {
      alert("请输入策略名称");
      return;
    }
    if (codeOverLimit) {
      alert(`策略代码超出 ${MAX_CODE_LENGTH} 字符限制，当前 ${code.length} 字符`);
      return;
    }
    const saved = await saveStrategy({
      id,
      name,
      description,
      code,
      type: "single_stock",
      params: Object.fromEntries(params.map((p) => [p.name, p.default])),
    });
    if (!id && saved.id) {
      navigate(`/strategies/${saved.id}`);
      return;
    }
    alert("保存成功");
  };

  const generate = async () => {
    if (!prompt.trim()) return;
    setGenerating(true);
    try {
      const data = await generateStrategy(prompt);
      setCode(data.generated_code);
    } catch (e: any) {
      alert(e.message || "AI生成失败");
    } finally {
      setGenerating(false);
    }
  };

  const validate = async () => {
    setValidStatus("idle");
    setValidMsg("验证中...");
    try {
      // 本地验证：尝试编译代码
      // 由于Python代码在浏览器中无法直接编译，这里做基础语法检查
      const hasClass = /class\s+\w+\s*\(.*BaseStrategy.*\)/.test(code);
      const hasOnBar = /def\s+on_bar\s*\(/.test(code);
      if (hasClass && hasOnBar) {
        setValidStatus("valid");
        setValidMsg("策略语法正确");
      } else {
        setValidStatus("invalid");
        setValidMsg("策略代码格式错误：需要包含继承 BaseStrategy 的类和 on_bar 方法");
      }
    } catch (e: any) {
      setValidStatus("invalid");
      setValidMsg(e.message || "验证失败");
    }
  };

  const toggleStock = (symbol: string) => {
    setSelectedStocks((prev) =>
      prev.includes(symbol) ? prev.filter((s) => s !== symbol) : [...prev, symbol]
    );
  };

  const runBacktest = async () => {
    if (!code.trim()) {
      alert("策略代码不能为空");
      return;
    }
    if (!fullMarketScan && selectedStocks.length === 0) {
      alert("请至少选择一只股票，或开启全市场扫描");
      return;
    }
    setBacktesting(true);
    try {
      const backtestParams: any = {
        strategy_code: code,
        symbols: fullMarketScan ? [] : selectedStocks,
        start_date: startDate,
        end_date: endDate,
        initial_cash: initialCash,
      };
      if (scope === "portfolio") {
        backtestParams.scope = "portfolio";
        backtestParams.params = {
          weight_mode: weightMode,
          rebalance_freq: rebalanceFreq,
          ...Object.fromEntries(params.map((p) => [p.name, p.default])),
        };
      } else if (scope === "scan" || fullMarketScan) {
        backtestParams.scope = "scan";
        backtestParams.params = {
          top_n: 50,
          score_threshold: 60,
          ...Object.fromEntries(params.map((p) => [p.name, p.default])),
        };
      } else {
        backtestParams.params = Object.fromEntries(params.map((p) => [p.name, p.default]));
      }
      const { data } = await backtestAPI.submit(backtestParams);
      navigate(`/backtests/${data.id}`);
    } catch (e: any) {
      alert(e.response?.data?.detail || "回测提交失败");
    } finally {
      setBacktesting(false);
    }
  };

  return (
    <div>
      <h1>{id ? "编辑策略" : "新建策略"}</h1>
      <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "1rem" }}>
        <input placeholder="策略名称" value={name} onChange={(e) => setName(e.target.value)} />
        <input placeholder="策略描述" value={description} onChange={(e) => setDescription(e.target.value)} />

        {/* AI 生成 */}
        <div className="card" style={{ display: "flex", gap: "0.5rem" }}>
          <input
            placeholder="用自然语言描述策略，例如：10日均线上穿30日均线买入..."
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            style={{ flex: 1 }}
          />
          <button onClick={generate} disabled={generating}>
            {generating ? "生成中..." : "AI生成代码"}
          </button>
        </div>

        {/* 代码编辑 */}
        <div style={{ position: "relative" }}>
          <CodeEditor
            value={code}
            onChange={(v) => {
              setCode(v);
              setValidStatus("idle");
            }}
            rows={16}
            maxLength={MAX_CODE_LENGTH}
          />
          <div style={{ marginTop: "0.5rem", display: "flex", gap: "0.5rem", alignItems: "center", justifyContent: "space-between" }}>
            <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
              <button className="secondary" onClick={validate} disabled={codeOverLimit}>
                验证代码
              </button>
              {validStatus === "valid" && (
                <span style={{ color: "#22c55e", fontSize: "0.875rem" }}>{validMsg}</span>
              )}
              {validStatus === "invalid" && (
                <span style={{ color: "#ef4444", fontSize: "0.875rem" }}>{validMsg}</span>
              )}
              {validStatus === "idle" && validMsg && (
                <span style={{ color: "var(--muted)", fontSize: "0.875rem" }}>{validMsg}</span>
              )}
            </div>
            <span style={{ fontSize: "0.75rem", color: codeOverLimit ? "#ef4444" : "var(--muted)" }}>
              {code.length} / {MAX_CODE_LENGTH}
            </span>
          </div>
        </div>

        {/* 参数面板 */}
        {params.length > 0 && (
          <div className="card">
            <h3 style={{ margin: "0 0 0.5rem 0" }}>策略参数</h3>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(180px, 1fr))", gap: "0.75rem" }}>
              {params.map((p) => (
                <div key={p.name}>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>{p.name}</label>
                  <input
                    type={p.type === "int" || p.type === "float" || p.type === "number" ? "number" : "text"}
                    value={p.default}
                    onChange={(e) => {
                      const val = p.type === "bool"
                        ? e.target.checked
                        : p.type === "int" || p.type === "float" || p.type === "number"
                        ? Number(e.target.value)
                        : e.target.value;
                      setParams((prev) => prev.map((x) => (x.name === p.name ? { ...x, default: val } : x)));
                    }}
                  />
                </div>
              ))}
            </div>
          </div>
        )}

        <div style={{ display: "flex", gap: "0.5rem" }}>
          <button onClick={save} disabled={codeOverLimit}>保存策略</button>
        </div>

        {/* 回测参数 */}
        {id && (
          <div className="card" style={{ marginTop: "1rem" }}>
            <h3>回测设置</h3>
            <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "0.75rem" }}>

              <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>回测模式:</label>
                <select value={scope} onChange={(e) => setScope(e.target.value as any)}>
                  <option value="single">个股回测</option>
                  <option value="portfolio">组合回测</option>
                  <option value="scan">全市场扫描</option>
                </select>
              </div>

              {scope === "portfolio" && (
                <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>权重:</label>
                  <select value={weightMode} onChange={(e) => setWeightMode(e.target.value)}>
                    <option value="equal">等权</option>
                    <option value="market_cap">市值加权</option>
                    <option value="custom">自定义</option>
                  </select>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>再平衡:</label>
                  <select value={rebalanceFreq} onChange={(e) => setRebalanceFreq(e.target.value)}>
                    <option value="1W">每周</option>
                    <option value="1M">每月</option>
                    <option value="3M">每季</option>
                    <option value="none">不再平衡</option>
                  </select>
                </div>
              )}

              <StockSelector
                selectedStocks={selectedStocks}
                onToggleStock={toggleStock}
                fullMarketScan={fullMarketScan}
                onFullMarketScanChange={setFullMarketScan}
              />

              <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "0.75rem" }}>
                <div>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>开始日期</label>
                  <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} />
                </div>
                <div>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>结束日期</label>
                  <input type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
                </div>
                <div>
                  <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>初始资金</label>
                  <input type="number" value={initialCash} onChange={(e) => setInitialCash(Number(e.target.value))} />
                </div>
              </div>

              <div style={{ display: "flex", gap: "0.5rem" }}>
                <button onClick={runBacktest} disabled={backtesting}>
                  {backtesting ? "提交中..." : "运行回测"}
                </button>
                <button className="secondary" onClick={() => navigate("/backtests")}>
                  查看回测列表
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
