import { isTauri, BrowserStore, IStore } from "./web-compat";

let storeInstance: IStore | null = null;

async function getStore(): Promise<IStore> {
  if (!storeInstance) {
    if (isTauri()) {
      const { Store } = await import("@tauri-apps/plugin-store");
      storeInstance = await Store.load("auth.json") as unknown as IStore;
    } else {
      storeInstance = new BrowserStore("auth");
    }
  }
  return storeInstance;
}

export async function getAccessToken(): Promise<string | null> {
  // 浏览器模式: token 在 httpOnly cookie 中，JS 不可读，返回 null
  if (!isTauri()) return null;
  try {
    const store = await getStore();
    const val = await store.get("access_token");
    return val || null;
  } catch (e) {
    console.error("Failed to get access_token:", e);
    return null;
  }
}

export async function getRefreshToken(): Promise<string | null> {
  // 浏览器模式: token 在 httpOnly cookie 中，JS 不可读，返回 null
  if (!isTauri()) return null;
  try {
    const store = await getStore();
    const val = await store.get("refresh_token");
    return val || null;
  } catch (e) {
    console.error("Failed to get refresh_token:", e);
    return null;
  }
}

export async function setTokens(access: string, refresh: string): Promise<void> {
  // 浏览器模式: token 由后端 httpOnly cookie 管理，无需 JS 存储
  if (!isTauri()) return;
  try {
    const store = await getStore();
    await store.set("access_token", access);
    await store.set("refresh_token", refresh);
    await store.save();
    broadcastTokenUpdate();
  } catch (e) {
    console.error("Failed to save tokens:", e);
    throw e;
  }
}

export async function clearTokens(): Promise<void> {
  try {
    if (isTauri()) {
      const store = await getStore();
      await store.delete("access_token");
      await store.delete("refresh_token");
      await store.save();
    }
    broadcastTokenUpdate();
  } catch (e) {
    console.error("Failed to clear tokens:", e);
  }
}

function broadcastTokenUpdate() {
  try {
    localStorage.setItem("token_sync", Date.now().toString());
  } catch (_) {}
}

export function onTokenSync(callback: () => void): () => void {
  const handler = (e: StorageEvent) => {
    if (e.key === "token_sync") {
      callback();
    }
  };
  window.addEventListener("storage", handler);
  return () => window.removeEventListener("storage", handler);
}
