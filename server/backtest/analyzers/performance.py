"""
绩效分析器：计算回测的各项风险收益指标。
兼容 Python 3.11.8
"""
from typing import List, Dict, Any
import pandas as pd
import numpy as np


class PerformanceAnalyzer:
    def calculate(
        self,
        trades: List[Any],
        equity_curve: List[Dict[str, Any]],
        initial_cash: float,
        benchmark_curve: List[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        if not equity_curve:
            return {"error": "No equity data"}

        df = pd.DataFrame(equity_curve)
        df["datetime"] = pd.to_datetime(df["datetime"])
        df.set_index("datetime", inplace=True)
        df["returns"] = df["total_value"].pct_change().fillna(0)

        total_return = (df["total_value"].iloc[-1] - initial_cash) / initial_cash
        # 使用交易日数量计算年化（A股一年约252个交易日）
        trading_days = max(len(df), 1)
        duration_years_trading = trading_days / 252
        annual_return = (1 + total_return) ** (1 / max(duration_years_trading, 1e-6)) - 1
        duration_days = max((df.index[-1] - df.index[0]).days, 1)

        # 最大回撤
        cummax = df["total_value"].cummax()
        drawdown = (df["total_value"] - cummax) / cummax
        max_drawdown = drawdown.min()

        # 夏普比率（假设无风险利率 2%）
        rf_daily = 0.02 / 252
        excess = df["returns"] - rf_daily
        sharpe = (excess.mean() / excess.std()) * np.sqrt(252) if excess.std() > 0 else 0

        # 索提诺比率（只考虑下行波动）
        downside = df["returns"][df["returns"] < 0]
        downside_std = downside.std() if len(downside) > 0 else 0
        sortino = (excess.mean() / downside_std) * np.sqrt(252) if downside_std > 0 else 0

        # Calmar 比率
        calmar = annual_return / abs(max_drawdown) if max_drawdown != 0 else float("inf")

        # Beta / Alpha（需要基准数据）
        beta = None
        alpha = None
        if benchmark_curve and len(benchmark_curve) > 0:
            bdf = pd.DataFrame(benchmark_curve)
            bdf["datetime"] = pd.to_datetime(bdf["datetime"])
            bdf.set_index("datetime", inplace=True)
            bdf["returns"] = bdf["total_value"].pct_change().fillna(0)
            # 对齐日期
            aligned = pd.merge(
                df[["returns"]], bdf[["returns"]],
                left_index=True, right_index=True, how="inner",
                suffixes=("_s", "_b"),
            )
            if len(aligned) >= 2:
                cov = aligned["returns_s"].cov(aligned["returns_b"])
                var_b = aligned["returns_b"].var()
                beta = cov / var_b if var_b > 0 else 0
                # Alpha = 策略年化收益 - 无风险利率 - Beta * (基准年化收益 - 无风险利率)
                bench_total = (bdf["total_value"].iloc[-1] - bdf["total_value"].iloc[0]) / bdf["total_value"].iloc[0] if len(bdf) > 0 else 0
                bench_annual = (1 + bench_total) ** (1 / max(duration_years_trading, 1e-6)) - 1
                alpha = annual_return - 0.02 - beta * (bench_annual - 0.02)

        # 胜率 & 盈亏比 - 按完整交易对计算（一次买入+一次卖出=一轮）
        from collections import defaultdict

        # 把 Trade 对象转成字典
        if trades and hasattr(trades[0], "symbol"):  # 是 Trade 对象列表
            trades = [
                {
                    "symbol": t.symbol,
                    "type": t.order_type.value,
                    "amount": t.amount,
                    "price": t.price,
                    "commission": t.commission,
                }
                for t in trades
            ]

        trade_df = pd.DataFrame(trades) if trades else pd.DataFrame({"pnl": []})

        if not trade_df.empty and len(trade_df) >= 2:
            # FIFO 精确盈亏配对：支持部分卖出（卖出数量可能大于单笔买入数量）
            paired_pnls = []
            # pending_buys[sym] = deque of {"price": float, "amount": float, "commission": float}
            from collections import defaultdict, deque

            pending_buys = defaultdict(deque)

            for t in trades:
                sym = t["symbol"]
                if t["type"] == "buy":
                    pending_buys[sym].append({
                        "price": float(t["price"]),
                        "amount": float(t["amount"]),
                        "commission": float(t.get("commission", 0)),
                    })
                else:  # sell
                    sell_price = float(t["price"])
                    sell_amount = float(t["amount"])
                    sell_comm = float(t.get("commission", 0))
                    remaining = sell_amount

                    while remaining > 0 and pending_buys[sym]:
                        buy = pending_buys[sym][0]
                        buy_amount = buy["amount"]
                        buy_price = buy["price"]
                        # 按比例分摊买入佣金
                        use_amount = min(remaining, buy_amount)
                        buy_comm_ratio = use_amount / buy_amount if buy_amount > 0 else 0
                        buy_comm = buy["commission"] * buy_comm_ratio
                        sell_comm_ratio = use_amount / sell_amount if sell_amount > 0 else 0
                        sell_comm_alloc = sell_comm * sell_comm_ratio

                        pnl = (sell_price - buy_price) * use_amount - buy_comm - sell_comm_alloc
                        paired_pnls.append(pnl)

                        remaining -= use_amount
                        buy["amount"] -= use_amount
                        if buy["amount"] <= 0:
                            pending_buys[sym].popleft()

            if paired_pnls:
                wins = [p for p in paired_pnls if p > 0]
                losses = [p for p in paired_pnls if p < 0]
                win_rate = len(wins) / len(paired_pnls) if len(paired_pnls) > 0 else 0
                avg_win = sum(wins) / len(wins) if wins else 0
                avg_loss = abs(sum(losses) / len(losses)) if losses else 0
                profit_ratio = avg_win / avg_loss if avg_loss > 0 else float("inf")
                total_trades = len(paired_pnls)
            else:
                win_rate = 0
                profit_ratio = 0
                total_trades = 0
            total_commission = sum(float(t.get("commission", 0)) for t in trades)
        else:
            win_rate = 0
            profit_ratio = 0
            total_trades = 0
            total_commission = 0

        # 月度收益分布
        monthly_returns = df["total_value"].resample("ME").last().pct_change().fillna(0)
        monthly_return_list = [
            {"month": d.strftime("%Y-%m"), "return": round(r, 4)}
            for d, r in monthly_returns.items()
        ]

        def _safe_json(val):
            if val is None:
                return None
            if isinstance(val, float) and (np.isinf(val) or np.isnan(val)):
                return None
            return val

        return {
            "total_return": _safe_json(round(total_return, 4)),
            "annual_return": _safe_json(round(annual_return, 4)),
            "max_drawdown": _safe_json(round(max_drawdown, 4)),
            "sharpe_ratio": _safe_json(round(sharpe, 4)),
            "sortino_ratio": _safe_json(round(sortino, 4)),
            "calmar_ratio": _safe_json(round(calmar, 4)),
            "win_rate": _safe_json(round(win_rate, 4)),
            "profit_ratio": _safe_json(round(profit_ratio, 4)),
            "total_trades": total_trades,
            "total_commission": _safe_json(round(total_commission, 2)),
            "final_value": _safe_json(round(df["total_value"].iloc[-1], 2)),
            "duration_days": int(duration_days),
            "trading_days": int(trading_days),
            "monthly_returns": monthly_return_list,
            "beta": _safe_json(round(beta, 4)) if beta is not None else None,
            "alpha": _safe_json(round(alpha, 4)) if alpha is not None else None,
        }
