import api from "./api";

export interface AIGenerateResult {
  generated_code: string;
  model: string;
  tokens_used?: number;
}

export async function generateStrategy(prompt: string): Promise<AIGenerateResult> {
  const { data } = await api.post("/ai/generate", {
    prompt,
    model: "deepseek-chat",
  });
  let generatedCode = data.generated_code || "";

  // 清理可能的 markdown 代码块
  if (generatedCode.startsWith("```python")) {
    generatedCode = generatedCode.replace(/^```python\n/, "").replace(/\n```$/, "");
  } else if (generatedCode.startsWith("```")) {
    generatedCode = generatedCode.replace(/^```\n/, "").replace(/\n```$/, "");
  }

  return {
    generated_code: generatedCode,
    model: data.model || "deepseek-chat",
    tokens_used: data.tokens_used,
  };
}

export async function extractParamsFromCode(code: string, _signal?: AbortSignal): Promise<{ params: Array<{ name: string; default: any; type: string }> }> {
  // 本地正则提取参数，不调用远程API
  const regex = /self\.params\.get\([\x27\x22]([\w\u4e00-\u9fff]+)[\x27\x22]\s*,\s*([^)]+)\)/g;
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
