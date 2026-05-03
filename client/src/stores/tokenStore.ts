import { Store } from "@tauri-apps/plugin-store";

let storeInstance: Store | null = null;

async function getStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = await Store.load("auth.json");
  }
  return storeInstance;
}

export async function getAccessToken(): Promise<string | null> {
  try {
    const store = await getStore();
    return (await store.get<string>("access_token")) || null;
  } catch (e) {
    console.error("Failed to get access_token:", e);
    return null;
  }
}

export async function getRefreshToken(): Promise<string | null> {
  try {
    const store = await getStore();
    return (await store.get<string>("refresh_token")) || null;
  } catch (e) {
    console.error("Failed to get refresh_token:", e);
    return null;
  }
}

export async function setTokens(access: string, refresh: string): Promise<void> {
  try {
    const store = await getStore();
    await store.set("access_token", access);
    await store.set("refresh_token", refresh);
    await store.save();
    // Sync to localStorage for PrivateRoute sync check
    localStorage.setItem("access_token", access);
    localStorage.setItem("refresh_token", refresh);
    broadcastTokenUpdate();
  } catch (e) {
    console.error("Failed to save tokens:", e);
    throw e;
  }
}

export async function clearTokens(): Promise<void> {
  try {
    const store = await getStore();
    await store.delete("access_token");
    await store.delete("refresh_token");
    await store.save();
    localStorage.removeItem("access_token");
    localStorage.removeItem("refresh_token");
    broadcastTokenUpdate();
  } catch (e) {
    console.error("Failed to clear tokens:", e);
  }
}

// Multi-tab sync: use localStorage as a signaling channel
function broadcastTokenUpdate() {
  try {
    localStorage.setItem("token_sync", Date.now().toString());
  } catch (e) {
    // localStorage may be unavailable in some contexts
  }
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
