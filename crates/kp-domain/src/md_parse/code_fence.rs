//! 代码区间识别（MD-WL-05 / MD-TAG-04 / MD-H-01 的共同前提）。
//!
//! 技术方案 §5.1：**先产出全文代码区间列表**，后续匹配时用二分查找判断「是否在代码里」。
//! 覆盖三类：围栏代码块（``` / ~~~，≥3 个）、缩进代码块（行首 4 空格或 Tab）、行内代码（`…`）。

use std::ops::Range;

/// 全文代码区间（字节偏移，左闭右开，按起点升序）。
#[derive(Debug, Default, Clone)]
pub struct CodeRanges {
    ranges: Vec<Range<usize>>,
}

impl CodeRanges {
    /// 扫描全文，产出代码区间列表。
    pub fn scan(text: &str) -> Self {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut fence: Option<(u8, usize, usize)> = None; // (字符, 长度, 起始字节)
        let mut indented_start: Option<usize> = None;
        let mut offset = 0usize;
        for line in text.split_inclusive('\n') {
            let start = offset;
            let end = offset + line.len();
            offset = end;
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if let Some((ch, len, fence_start)) = fence {
                let t = trimmed.trim_start();
                let count = t.bytes().take_while(|b| *b == ch).count();
                if count >= len && t[count..].trim().is_empty() {
                    ranges.push(fence_start..end);
                    fence = None;
                }
                continue;
            }
            let t = trimmed.trim_start();
            let ticks = t.bytes().take_while(|b| *b == b'`').count();
            let tildes = t.bytes().take_while(|b| *b == b'~').count();
            if ticks >= 3 || tildes >= 3 {
                let (ch, len) = if ticks >= 3 {
                    (b'`', ticks)
                } else {
                    (b'~', tildes)
                };
                fence = Some((ch, len, start));
                if let Some(s) = indented_start.take() {
                    ranges.push(s..start);
                }
                continue;
            }
            let is_indented = line.starts_with("    ") || line.starts_with('\t');
            if is_indented {
                if indented_start.is_none() {
                    indented_start = Some(start);
                }
            } else {
                if let Some(s) = indented_start.take() {
                    ranges.push(s..start);
                }
                // 行内代码：同一行内成对的 `…`（成对的 ``…`` 亦覆盖）
                collect_inline_code(trimmed, start, &mut ranges);
            }
        }
        if let Some((_, _, s)) = fence {
            ranges.push(s..text.len());
        }
        if let Some(s) = indented_start {
            ranges.push(s..text.len());
        }
        ranges.sort_by_key(|r| r.start);
        Self { ranges }
    }

    /// 字节位置是否在代码区间内（二分定位，O(log n)）。
    pub fn contains(&self, byte: usize) -> bool {
        let i = self.ranges.partition_point(|r| r.start <= byte);
        i > 0 && self.ranges[i - 1].end > byte
    }

    /// 区间列表（只读，供测试与诊断）。
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }
}

/// 收集一行内的行内代码区间（`x` 与 ``x``）。
fn collect_inline_code(line: &str, base: usize, out: &mut Vec<Range<usize>>) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let mut run = 0usize;
        while i + run < bytes.len() && bytes[i + run] == b'`' {
            run += 1;
        }
        // 找等长的收尾反引号串
        let mut j = i + run;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let mut run2 = 0usize;
                while j + run2 < bytes.len() && bytes[j + run2] == b'`' {
                    run2 += 1;
                }
                if run2 == run {
                    closed = Some(j + run2);
                    break;
                }
                j += run2;
            } else {
                j += 1;
            }
        }
        match closed {
            Some(close) => {
                out.push((base + i)..(base + close));
                i = close;
            }
            None => i += run,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fenced_code_block() {
        let text = "before\n```ts\n[[Note]]\n```\nafter\n";
        let r = CodeRanges::scan(text);
        let inside = text.find("[[Note]]").expect("位置");
        assert!(r.contains(inside), "围栏内应判为代码");
        let outside = text.find("before").expect("位置");
        assert!(!r.contains(outside));
    }

    #[test]
    fn detects_tilde_fence_and_unclosed_fence() {
        let text = "~~~\n[[A]]\n";
        let r = CodeRanges::scan(text);
        let inside = text.find("[[A]]").expect("位置");
        assert!(r.contains(inside), "未闭合围栏应延伸到文末（与渲染器一致）");
    }

    #[test]
    fn detects_indented_code_block() {
        let text = "text\n    [[Indented]]\nmore\n";
        let r = CodeRanges::scan(text);
        assert!(r.contains(text.find("[[Indented]]").expect("位置")));
        assert!(!r.contains(text.find("more").expect("位置")));
    }

    #[test]
    fn detects_inline_code() {
        let text = "a `[[Inline]]` b\n";
        let r = CodeRanges::scan(text);
        assert!(r.contains(text.find("[[Inline]]").expect("位置")));
        assert!(!r.contains(text.find(" b").expect("位置")));
    }
}
