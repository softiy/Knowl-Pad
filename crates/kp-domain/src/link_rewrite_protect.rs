//! 受保护区域扫描：哪些字节区间内的链接**一律不得改写**（AC-FILE-01 的硬要求）。
//!
//! 四类：frontmatter、围栏代码块（反引号或波浪线）、行内代码、HTML 注释。
//! 从 link_rewrite.rs 拆出，使两个文件都低于 CODE-11 的 200 行警告线。

const TICK: u8 = 0x60;
const TILDE: u8 = 0x7e;

/// 受保护区域的字节区间（左闭右开）。
pub fn protected_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();

    // frontmatter：只在文件开头
    if content.starts_with("---") {
        let mut idx = 3usize;
        let mut first = true;
        for line in content.split_inclusive('\n') {
            idx += line.len();
            if first {
                first = false;
                continue;
            }
            if line.trim_end() == "---" {
                out.push((0, idx));
                break;
            }
        }
    }

    // 围栏代码块：逐行切换状态
    let mut idx2 = 0usize;
    let mut fence_start: Option<usize> = None;
    for line in content.split_inclusive('\n') {
        let line_start = idx2;
        idx2 += line.len();
        let bytes = line.as_bytes();
        let mut lead = 0usize;
        while lead < bytes.len() && (bytes[lead] == b' ' || bytes[lead] == b'\t') {
            lead += 1;
        }
        let is_fence = bytes.len() >= lead + 3
            && ((bytes[lead] == TICK && bytes[lead + 1] == TICK && bytes[lead + 2] == TICK)
                || (bytes[lead] == TILDE && bytes[lead + 1] == TILDE && bytes[lead + 2] == TILDE));
        if is_fence {
            match fence_start {
                Some(open_at) => {
                    out.push((open_at, idx2));
                    fence_start = None;
                }
                None => fence_start = Some(line_start),
            }
        }
    }
    if let Some(s) = fence_start {
        out.push((s, content.len()));
    }

    // 行内代码 与 HTML 注释：逐行，跳过已在代码块里的行
    let mut idx3 = 0usize;
    for line in content.split_inclusive('\n') {
        let line_start = idx3;
        idx3 += line.len();
        if out.iter().any(|(s, e)| line_start >= *s && line_start < *e) {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == TICK {
                let mut run = 0usize;
                while i + run < bytes.len() && bytes[i + run] == TICK {
                    run += 1;
                }
                let mut j = i + run;
                let mut closed: Option<usize> = None;
                while j < bytes.len() {
                    if bytes[j] == TICK {
                        let mut run2 = 0usize;
                        while j + run2 < bytes.len() && bytes[j + run2] == TICK {
                            run2 += 1;
                        }
                        if run2 == run {
                            closed = Some(j + run);
                            break;
                        }
                        j += run2;
                    } else {
                        j += 1;
                    }
                }
                if let Some(end) = closed {
                    out.push((line_start + i, line_start + end));
                    i = end;
                    continue;
                }
            }
            if bytes[i..].starts_with(b"<!--") {
                let rest = &line[i..];
                let end_rel = rest.find("-->").map(|p| p + 3).unwrap_or(rest.len());
                out.push((line_start + i, line_start + i + end_rel));
                i += end_rel;
                continue;
            }
            i += 1;
        }
    }

    out.sort();
    out
}

pub fn in_protected(ranges: &[(usize, usize)], start: usize, end: usize) -> bool {
    ranges.iter().any(|(s, e)| start < *e && end > *s)
}
