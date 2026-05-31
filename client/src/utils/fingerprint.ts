/**
 * 设备指纹工具
 * - Tauri: 使用原生 fingerprint
 * - 浏览器: 使用 FingerprintJS 生成稳定 visitorId，缓存到 localStorage
 */
import { isTauri } from "../stores/web-compat";

const STORAGE_KEY = "acda_device_fp";

let cachedFingerprint: string | null = null;

/**
 * 获取设备指纹（稳定、跨会话持久）
 */
export async function getDeviceFingerprint(): Promise<string> {
  // 内存缓存
  if (cachedFingerprint) return cachedFingerprint;

  // Tauri 环境使用原生接口
  if (isTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const fp = await invoke<string>("get_device_fingerprint");
      cachedFingerprint = fp;
      return fp;
    } catch (_) {
      // fall through to browser fingerprinting
    }
  }

  // 浏览器环境: 先检查 localStorage 缓存
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && stored.length > 10) {
      cachedFingerprint = stored;
      return stored;
    }
  } catch (_) {}

  // 使用 FingerprintJS 生成稳定指纹
  try {
    const FingerprintJS = await import("@fingerprintjs/fingerprintjs");
    const fp = await FingerprintJS.load();
    const result = await fp.get();
    const visitorId = result.visitorId;
    cachedFingerprint = visitorId;
    try {
      localStorage.setItem(STORAGE_KEY, visitorId);
    } catch (_) {}
    return visitorId;
  } catch (e) {
    console.warn("[Fingerprint] FingerprintJS failed, using fallback:", e);
    // 降级: 基于较少变化的特征生成简单指纹
    const fallback = await generateFallbackFingerprint();
    cachedFingerprint = fallback;
    try {
      localStorage.setItem(STORAGE_KEY, fallback);
    } catch (_) {}
    return fallback;
  }
}

/**
 * 降级指纹: 基于相对稳定的浏览器特征
 * 不包含 UA 版本号和精确屏幕分辨率（这些变化太频繁）
 */
async function generateFallbackFingerprint(): Promise<string> {
  const canvas = getCanvasFingerprint();
  const raw = [
    navigator.platform,
    navigator.hardwareConcurrency,
    navigator.maxTouchPoints,
    Intl.DateTimeFormat().resolvedOptions().timeZone,
    canvas,
  ].join("|");

  // 简单 hash
  const msgUint8 = new TextEncoder().encode(raw);
  const hashBuffer = await crypto.subtle.digest("SHA-256", msgUint8);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
}

function getCanvasFingerprint(): string {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = 200;
    canvas.height = 50;
    const ctx = canvas.getContext("2d");
    if (!ctx) return "no-canvas";
    ctx.textBaseline = "top";
    ctx.font = "14px Arial";
    ctx.fillStyle = "#f60";
    ctx.fillRect(125, 1, 62, 20);
    ctx.fillStyle = "#069";
    ctx.fillText("ACDA-Quant", 2, 15);
    return canvas.toDataURL().slice(-50);
  } catch {
    return "canvas-error";
  }
}
