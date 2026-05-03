import { useAISettingsStore } from "../stores/aiSettingsStore";

const SYSTEM_PROMPT = `你是一个专业的量化交易策略生成助手。请根据用户的自然语言描述，生成符合以下框架的Python策略代码。

框架要求：
1. 策略类必须继承 BaseStrategy
2. 必须实现 on_bar(self, context, bar_group) 方法
3. 使用 context.buy(symbol, percent=0.1) 买入（percent 为资金百分比）
4. 使用 context.sell(symbol, percent=1.0) 卖出（percent 为持仓百分比）
5. 使用 self.params.get("参数名", 默认值) 获取策略参数
6. bar_group 是 pandas DataFrame，包含多只股票的数据，可通过 bar_group["symbol"].unique() 获取所有股票
7. 单只股票数据通过 bar_group[bar_group["symbol"] == symbol] 过滤
8. 可用字段：open, high, low, close, volume

请只返回Python代码，不要包含任何解释、markdown代码块标记或其他文本。代码必须可直接运行。`;

export interface AIGenerateResult {
  generated_code: string;
  model: string;
  tokens_used?: number;
}

export async function generateStrategy(prompt: string): Promise<AIGenerateResult> {
  const { deepseekApiKey, deepseekBaseUrl, deepseekModel } = useAISettingsStore.getState();

  if (!deepseekApiKey.trim()) {
    throw new Error("请先设置 DeepSeek API Key");
  }

  const response = await fetch(`${deepseekBaseUrl}/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Authorization": `Bearer ${deepseekApiKey}`,
    },
    body: JSON.stringify({
      model: deepseekModel,
      messages: [
        { role: "system", content: SYSTEM_PROMPT },
        { role: "user", content: prompt },
      ],
      temperature: 0.3,
      max_tokens: 4096,
    }),
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: { message: `HTTP ${response.status}` } }));
    throw new Error(error.error?.message || `DeepSeek API 请求失败: ${response.status}`);
  }

  const data = await response.json();
  const generatedCode = data.choices?.[0]?.message?.content?.trim() || "";

  // 清理可能的 markdown 代码块
  let cleanCode = generatedCode;
  if (cleanCode.startsWith("```python")) {
    cleanCode = cleanCode.replace(/^```python\n/, "").replace(/\n```$/, "");
  } else if (cleanCode.startsWith("```")) {
    cleanCode = cleanCode.replace(/^```\n/, "").replace(/\n```$/, "");
  }

  return {
    generated_code: cleanCode,
    model: data.model || deepseekModel,
    tokens_used: data.usage?.total_tokens,
  };
}

export async function extractParamsFromCode(code: string): Promise<{ params: Array<{ name: string; default: any; type: string }> }> {
  // 本地正则提取参数，不调用远程API
  const regex = /self\.params\.get\(["'](\w+)["']\s*,\s*([^)]+)\)/g;
  const found: Array<{ name: string; default: any; type: string }> = [];
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
  return { params: found };
}
