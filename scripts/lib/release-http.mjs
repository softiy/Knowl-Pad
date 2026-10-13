// Gitee OpenAPI 的 HTTP 出口（抽取自 publish-gitee-release.mjs）。
// 负责：超时、5xx 重试、统一失败出口。
// 注意 fail() 只打印并抛错，不 process.exit()：带未关闭的 HTTP socket 硬退出在 Windows 上会 fail-fast（0xC0000409）。

export const API = "https://gitee.com/api/v5";
export const MAX_RETRY = 3;

let reported = false;

export function hasReported() {
  return reported;
}

export function fail(msg, detail) {
  reported = true;
  console.error("❌ " + msg);
  if (detail !== undefined) console.error(typeof detail === "string" ? detail : JSON.stringify(detail, null, 2));
  throw new Error(msg);
}

export async function request(url, init, timeout, label) {
  for (let attempt = 1; attempt <= MAX_RETRY; attempt++) {
    const ac = new AbortController();
    const timer = setTimeout(() => ac.abort(), timeout);
    try {
      const res = await fetch(url, { ...init, signal: ac.signal });
      clearTimeout(timer);
      const text = await res.text();
      if (res.status >= 500 && attempt < MAX_RETRY) {
        console.error("  ↻ " + label + " 第 " + attempt + " 次失败（HTTP " + res.status + "），重试…");
        await new Promise((r) => setTimeout(r, 1500 * attempt));
        continue;
      }
      return { status: res.status, text };
    } catch (err) {
      clearTimeout(timer);
      if (attempt < MAX_RETRY) { await new Promise((r) => setTimeout(r, 1500 * attempt)); continue; }
      throw err;
    }
  }
  throw new Error(label + " 重试耗尽");
}
