"""
全市场扫描器：对全市场标的逐只运行策略，输出信号列表、策略适应性评分等。
兼容 Python 3.11.8
"""
from typing import List, Dict, Any, Optional
from dataclasses import dataclass
import pandas as pd
import numpy as np

from server.backtest.engine.core import BacktestEngine
from server.backtest.sandbox.executor import compile_strategy_code, load_strategy_class
from server.backtest.analyzers.performance import PerformanceAnalyzer


@dataclass
class ScanSignal:
    symbol: str
    direction: str  # buy / sell / hold
    timestamp: str
    price: float
    score: float  # 策略对该标的的适应性评分 0-100


@dataclass
class ScanResult:
    signals: List[ScanSignal]
    overall_performance: Dict[str, Any]
    suitable_stocks: List[Dict[str, Any]]  # 适合该策略的股票列表（含评分）
    unsuitable_stocks: List[Dict[str, Any]]  # 不适合的股票列表


class MarketScanner:
    """
    全市场策略扫描器。
    对每个标的单独运行回测，综合评估策略适用性。
    """

    def __init__(
        self,
        code: str,
        start_date: str,
        end_date: str,
        initial_cash: float = 100_000.0,
        top_n: int = 50,
        score_threshold: float = 60.0,
    ):
        self.code = code
        self.start_date = start_date
        self.end_date = end_date
        self.initial_cash = initial_cash
        self.top_n = top_n
        self.score_threshold = score_threshold

    def run(
        self,
        symbols: List[str],
        params: Optional[Dict[str, Any]] = None,
    ) -> ScanResult:
        """
        对 symbols 列表逐只运行策略回测，收集结果。
        策略代码只编译一次，避免重复开销。
        """
        # 编译策略代码一次（缓存）
        try:
            module = compile_strategy_code(self.code)
            strategy_cls = load_strategy_class(module)
        except Exception:
            # 策略代码本身有问题，直接返回空结果
            return ScanResult(
                signals=[],
                overall_performance={},
                suitable_stocks=[],
                unsuitable_stocks=[],
            )

        per_symbol_results: List[Dict[str, Any]] = []
        all_signals: List[ScanSignal] = []

        for symbol in symbols:
            try:
                engine = BacktestEngine(
                    initial_cash=self.initial_cash,
                )
                engine.set_strategy(strategy_cls, params=params)
                result = engine.run([symbol], self.start_date, self.end_date)
                if "error" in result:
                    continue

                # 从 engine 中提取完整的交易记录和净值曲线
                trades = [
                    {
                        "symbol": t.symbol,
                        "amount": t.amount,
                        "price": round(t.price, 2),
                        "type": t.order_type.value,
                        "timestamp": t.timestamp.isoformat(),
                        "commission": round(t.commission, 4),
                        "pnl": round(t.pnl, 2),
                    }
                    for t in engine.broker.trades
                ]
                result["trades"] = trades
                result["equity_curve"] = engine.equity_curve
                result["total_trades"] = len(engine.broker.trades)

                score = self._calculate_score(result)
                per_symbol_results.append({
                    "symbol": symbol,
                    "score": score,
                    "total_return": result.get("total_return", 0),
                    "max_drawdown": result.get("max_drawdown", 0),
                    "sharpe_ratio": result.get("sharpe_ratio", 0),
                    "total_trades": result.get("total_trades", 0),
                    "trades": trades,
                    "equity_curve": engine.equity_curve,
                })

                # 提取最近一条交易信号
                if trades:
                    last_trade = trades[-1]
                    all_signals.append(ScanSignal(
                        symbol=symbol,
                        direction=last_trade.get("type", "hold"),
                        timestamp=last_trade.get("timestamp", ""),
                        price=last_trade.get("price", 0.0),
                        score=score,
                    ))
            except Exception:
                # 单只标的不影响整体扫描
                continue

        # 计算整体表现（等权合成净值曲线）
        overall = self._compute_overall(per_symbol_results)

        # 排序并分类
        sorted_results = sorted(per_symbol_results, key=lambda x: x["score"], reverse=True)
        suitable = [
            {
                "symbol": r["symbol"],
                "score": round(r["score"], 2),
                "total_return": r["total_return"],
                "max_drawdown": r["max_drawdown"],
                "sharpe_ratio": r["sharpe_ratio"],
                "total_trades": r["total_trades"],
            }
            for r in sorted_results[:self.top_n] if r["score"] >= self.score_threshold
        ]
        unsuitable = [
            {
                "symbol": r["symbol"],
                "score": round(r["score"], 2),
                "total_return": r["total_return"],
                "max_drawdown": r["max_drawdown"],
            }
            for r in sorted_results[self.top_n:]
        ]

        return ScanResult(
            signals=all_signals,
            overall_performance=overall,
            suitable_stocks=suitable,
            unsuitable_stocks=unsuitable,
        )

    def _calculate_score(self, result: Dict[str, Any]) -> float:
        """
        计算策略对单只标的的适应性评分（0-100）。
        基于：收益率(30%)、夏普比率(25%)、回撤控制(25%)、交易活跃度(20%)。
        """
        total_return = result.get("total_return", 0) or 0
        sharpe = result.get("sharpe_ratio", 0) or 0
        max_dd = result.get("max_drawdown", 0) or 0
        trades = result.get("total_trades", 0) or 0

        # 收益率得分：年化收益 20% = 满分 30
        return_score = min(max(total_return * 30 / 0.20, 0), 30) if total_return > 0 else 0

        # 夏普得分：sharpe 2.0 = 满分 25
        sharpe_score = min(max(sharpe * 25 / 2.0, 0), 25)

        # 回撤得分：max_drawdown 0% = 满分 25，-30% = 0
        dd_score = min(max((abs(max_dd) - 0.30) / (0 - 0.30) * 25, 0), 25)

        # 交易活跃度得分：10笔以上 = 满分 20
        trade_score = min(max(trades * 20 / 10, 0), 20)

        return return_score + sharpe_score + dd_score + trade_score

    def _compute_overall(self, results: List[Dict[str, Any]]) -> Dict[str, Any]:
        """计算所有标的等权合成的整体策略表现"""
        if not results:
            return {}

        returns = [r["total_return"] for r in results]
        sharpes = [r["sharpe_ratio"] for r in results]
        drawdowns = [r["max_drawdown"] for r in results]
        trades = [r["total_trades"] for r in results]

        return {
            "avg_return": round(np.mean(returns), 4) if returns else 0,
            "median_return": round(np.median(returns), 4) if returns else 0,
            "avg_sharpe": round(np.mean(sharpes), 4) if sharpes else 0,
            "avg_drawdown": round(np.mean(drawdowns), 4) if drawdowns else 0,
            "total_signals": sum(trades),
            "scanned_count": len(results),
            "win_rate": round(sum(1 for r in returns if r > 0) / len(returns), 4) if returns else 0,
        }
