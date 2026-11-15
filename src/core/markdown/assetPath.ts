/**
 * 本地附件的**路径解析**（E1 / FR-EDITOR-43）—— 纯函数，不碰 DOM、不碰 IPC。
 *
 * 为什么单独成模块：阅读态图片有两段完全不同的职责 ——
 * ① "相对当前笔记的 src ⇒ Vault 内绝对路径" 是**纯路径运算**（本模块，可穷举单测）；
 * ② "绝对路径 ⇒ asset:// URL" 是 Tauri 运行时能力（`core/ipc/assetCommands.ts`）。
 * 混在一起就只能在真机上验证；拆开后路径安全（穿越/绝对路径/协议）可以用用例钉住。
 *
 * 安全口径（与 Rust 侧 PathGuard 同一方向，但**这里不是安全边界**：
 * 真正的写/读校验在 Rust；这里只保证"不把明显越界的路径交给 asset 协议"）：
 * - 远程 / 协议 URL（`http:`、`https:`、`data:`、`asset:`、`file:`、协议相对 `//host`）一律 null；
 * - 绝对路径（`/x`、`C:\x`、`\\\\server`）一律 null；
 * - `..` 越过 Vault 根 ⇒ null（不悄悄夹回根目录）；
 * - 百分号编码（markdown-it 会把空格/非 ASCII 编码）与反斜杠在这里归一。
 */

/** 是否远程 / 带协议的 URL（含协议相对 `//host`）。Windows 盘符 `C:` 也会命中 scheme 形态，**这是有意的**。 */
function isRemoteOrProtocol(value: string): boolean {
  if (value.startsWith('//') || value.startsWith('\\\\')) return true;
  return /^[a-z][a-z0-9+.-]*:/i.test(value);
}

/**
 * 百分号解码（一次）。非法序列（裸 `%`）按原文字面量处理 ——
 * 文件名为 `100%.png` 时不该让整篇渲染抛异常；是否真实存在由加载侧决定。
 */
function decodePath(raw: string): string {
  try {
    return decodeURIComponent(raw);
  } catch {
    // 不是 URL 错误而是文件名里可能有裸 %：降级为原文，是否可读交给加载侧
    return raw;
  }
}

/** 是否含控制字符（NUL / 换行等）：这种"路径"不可能是真实文件，且不应流进 asset URL。 */
function hasControlChars(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/**
 * 把相对路径拆成段并处理 `.`/`..`；越出根（`..` 把栈弹空）⇒ null。
 *
 * `base` 是"起点段"（当前笔记所在目录），`relative` 是正文里的 src。
 * 分开传是为了让调用方不需要先拼字符串再解析 —— 拼接会掩盖 `..` 的越界方向。
 */
function joinSegments(base: string[], relative: string): string[] | null {
  const stack = [...base];
  for (const segment of relative.split('/')) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') {
      if (stack.length === 0) return null;
      stack.pop();
      continue;
    }
    stack.push(segment);
  }
  return stack;
}

/** 笔记 relPath ⇒ 所在目录的段（`notes/sub/a.md` ⇒ `notes/sub`；空路径 / 根部 ⇒ 空数组）。 */
function noteDirSegments(noteRelPath: string): string[] | null {
  const normalized = noteRelPath.replace(/\\/g, '/');
  const slash = normalized.lastIndexOf('/');
  const dir = slash < 0 ? '' : normalized.slice(0, slash);
  return joinSegments([], dir);
}

/**
 * 解析阅读态本地图片的 src 为**绝对路径**（交给 asset 协议读取）。
 *
 * @param vaultRoot 当前 Vault 的绝对根目录（来自 `vault_current`，如 `C:\\Users\\me\\vault`）
 * @param noteRelPath 当前笔记的 Vault 相对路径（如 `notes/a.md`）
 * @param src Markdown 里的图片源（如 `./img/a.png`、`../assets/中文 图.png`）
 * @returns 绝对路径字符串；无法/不允许解析时 `null`（渲染器保留原 src，且仍不加载远程资源）
 */
export function resolveAssetSrc(vaultRoot: string, noteRelPath: string, src: string): string | null {
  const root = vaultRoot.trim();
  const raw = src.trim();
  if (!root || !raw) return null;
  // 先看原文再看解码后：`https%3A%2F%2F...` 解码后是协议 URL，也不能当本地路径
  if (isRemoteOrProtocol(raw)) return null;
  const decoded = decodePath(raw);
  if (decoded === '' || hasControlChars(decoded)) return null;
  if (isRemoteOrProtocol(decoded) || decoded.startsWith('/')) return null;

  const base = noteDirSegments(noteRelPath);
  if (base === null) return null;
  const segments = joinSegments(base, decoded);
  if (segments === null || segments.length === 0) return null;

  // 分隔符跟随 Vault 根（Windows 用 `\\`，其余用 `/`）：Windows 的 convertFileSrc 会把
  // 反斜杠编码成 %5C，asset 协议按平台语义还原；混用分隔符虽然多数能读，但没必要赌。
  const sep = root.includes('\\') ? '\\' : '/';
  const trimmedRoot = root.replace(/[\\/]+$/, '');
  // 根目录本身是 `/` 或盘符根 `C:\`：去掉尾部分隔符会变成相对路径，必须补回来
  const absoluteRoot =
    trimmedRoot === '' ? sep : /^[A-Za-z]:$/.test(trimmedRoot) ? trimmedRoot + sep : trimmedRoot;
  const rel = segments.join(sep);
  return absoluteRoot.endsWith(sep) ? absoluteRoot + rel : absoluteRoot + sep + rel;
}
