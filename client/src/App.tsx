import { Routes, Route } from "react-router-dom";
import { useEffect } from "react";
import Layout from "./components/Layout";
import PrivateRoute from "./components/PrivateRoute";
import ErrorBoundary from "./components/ErrorBoundary";
import LoginPage from "./pages/LoginPage";
import StrategyListPage from "./pages/StrategyListPage";
import StrategyEditorPage from "./pages/StrategyEditorPage";
import BacktestListPage from "./pages/BacktestListPage";
import BacktestResultPage from "./pages/BacktestResultPage";
import StrategyBacktestPage from "./pages/StrategyBacktestPage";
import DashboardPage from "./pages/DashboardPage";
import SettingsPage from "./pages/SettingsPage";
import PrivacyPolicyPage from "./pages/PrivacyPolicyPage";
import ProductInfoPage from "./pages/ProductInfoPage";
import SubscriptionPage from "./pages/SubscriptionPage";
import { useAuthStore } from "./stores/authStore";

function AuthInitializer({ children }: { children: React.ReactNode }) {
  const init = useAuthStore((s) => s.init);
  useEffect(() => {
    init();
  }, [init]);
  return <>{children}</>;
}

function App() {
  return (
    <AuthInitializer>
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/privacy" element={<PrivacyPolicyPage />} />
      <Route path="/product" element={<ProductInfoPage />} />
      <Route path="/" element={<PrivateRoute><ErrorBoundary><Layout /></ErrorBoundary></PrivateRoute>}>
        <Route index element={<DashboardPage />} />
        <Route path="strategies" element={<StrategyListPage />} />
        <Route path="strategies/new" element={<StrategyEditorPage />} />
        <Route path="strategies/:id" element={<StrategyEditorPage />} />
        <Route path="strategy-backtest" element={<StrategyBacktestPage />} />
        <Route path="backtests" element={<BacktestListPage />} />
        <Route path="backtests/:id" element={<BacktestResultPage />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="subscription" element={<SubscriptionPage />} />
      </Route>
    </Routes>
    </AuthInitializer>
  );
}

export default App;
