/**
 * 浏览器兼容层 — 当在浏览器（非 Tauri）中运行时，替换 Tauri 专有 API
 */

export function isTauri(): boolean {
  return typeof window !== 'undefined' && (window as any).__TAURI__ !== undefined;
}

export interface IStore {
  get(key: string): Promise<string | undefined>;
  set(key: string, value: unknown): Promise<void>;
  delete(key: string): Promise<void>;
  save(): Promise<void>;
}

/**
 * 浏览器环境下的 localStorage 存储，替代 @tauri-apps/plugin-store
 *
 * 注意：set() 使用 JSON.stringify 编码，get() 必须 JSON.parse 解码以保持数据一致性
 */
export class BrowserStore implements IStore {
  private prefix: string;

  constructor(_name: string) {
    this.prefix = `acda_${_name}_`;
  }

  async get(key: string): Promise<string | undefined> {
    const raw = localStorage.getItem(this.prefix + key);
    if (raw === null) return undefined;
    try {
      const parsed = JSON.parse(raw);
      return typeof parsed === 'string' ? parsed : raw;
    } catch {
      return raw;
    }
  }

  async set(key: string, value: unknown): Promise<void> {
    localStorage.setItem(this.prefix + key, JSON.stringify(value));
  }

  async delete(key: string): Promise<void> {
    localStorage.removeItem(this.prefix + key);
  }

  async save(): Promise<void> {
  }
}
