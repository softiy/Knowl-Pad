//! 索引库 DDL（CODE-11 豁免的 DDL 定义文件）。
//!
//! **权威出处：PRD §3.2.1**——不得自行增删字段。

pub const DDL_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file (
    id           INTEGER PRIMARY KEY,
    rel_path     TEXT    NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    stem         TEXT    NOT NULL,
    ext          TEXT    NOT NULL,
    kind         TEXT    NOT NULL,
    size_bytes   INTEGER NOT NULL,
    mtime_ms     INTEGER NOT NULL,
    content_hash TEXT,
    deleted      INTEGER NOT NULL DEFAULT 0,
    indexed_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_file_stem       ON file(stem);
CREATE INDEX IF NOT EXISTS idx_file_stem_lower ON file(lower(stem));
CREATE INDEX IF NOT EXISTS idx_file_kind       ON file(kind);
CREATE INDEX IF NOT EXISTS idx_file_deleted    ON file(deleted);

CREATE TABLE IF NOT EXISTS heading (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    level        INTEGER NOT NULL,
    text         TEXT    NOT NULL,
    anchor       TEXT    NOT NULL,
    line         INTEGER NOT NULL,
    sort_order   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_heading_file ON heading(file_id);

CREATE TABLE IF NOT EXISTS block_id (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    bid          TEXT    NOT NULL,
    line_start   INTEGER NOT NULL,
    line_end     INTEGER NOT NULL,
    UNIQUE(file_id, bid)
);
CREATE INDEX IF NOT EXISTS idx_block_file ON block_id(file_id);

CREATE TABLE IF NOT EXISTS link (
    id             INTEGER PRIMARY KEY,
    src_file_id    INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    dst_file_id    INTEGER,
    target_ref     TEXT    NOT NULL,
    anchor         TEXT,
    alias          TEXT,
    link_kind      TEXT    NOT NULL,
    status         TEXT    NOT NULL,
    line           INTEGER NOT NULL,
    col            INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_link_src    ON link(src_file_id);
CREATE INDEX IF NOT EXISTS idx_link_dst    ON link(dst_file_id);
CREATE INDEX IF NOT EXISTS idx_link_status ON link(status);

CREATE TABLE IF NOT EXISTS tag (
    id           INTEGER PRIMARY KEY,
    norm         TEXT    NOT NULL UNIQUE,
    display      TEXT    NOT NULL,
    depth        INTEGER NOT NULL,
    is_leaf      INTEGER NOT NULL DEFAULT 1,
    ref_count    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS file_tag (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    tag_id       INTEGER NOT NULL REFERENCES tag(id)  ON DELETE CASCADE,
    line         INTEGER NOT NULL,
    col          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_filetag_file ON file_tag(file_id);
CREATE INDEX IF NOT EXISTS idx_filetag_tag  ON file_tag(tag_id);

CREATE TABLE IF NOT EXISTS file_alias (
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    alias        TEXT    NOT NULL,
    PRIMARY KEY (file_id, alias)
);
CREATE INDEX IF NOT EXISTS idx_alias_lower ON file_alias(lower(alias));

-- 普通（自带内容）FTS5 表：增量索引需要 DELETE FROM note_fts WHERE rowid = ?（技术方案 §5.3.4）
CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(
    plain_text,
    tokenize='unicode61'
);
"#;
