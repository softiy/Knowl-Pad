//! 回收站的判据（FR-TRASH-12：清单是**权威来源**，绝不出现「文件已从原位置消失、而清单里没有它」）。
//!
//! 此前 `file_trash.rs` 没有任何直接测试，只被 file_ops 间接覆盖 —— 审计 R4 的"谎报"分支因此长期无人发现。

use kp_domain::file_trash::move_to_trash;

/// **审计 R4 的判据**：清单写入失败时，必须**真的把文件移回原位**，且文案如实说「未删除任何文件」。
///
/// 注入方式：把 manifest.json 预建成**目录** —— 打开它以追加必然失败，且在所有平台都确定。
#[test]
fn manifest_write_failure_rolls_back_move() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    std::fs::write(root.join("a.md"), "# 甲\n").expect("写笔记");
    let trash_dir = root.join(".knowlpad/trash");
    std::fs::create_dir_all(&trash_dir).expect("建回收站目录");
    std::fs::create_dir_all(trash_dir.join("manifest.json")).expect("把清单路径占成目录");

    let err =
        move_to_trash(root, &root.join("a.md"), "a.md", false).expect_err("清单写不进去时必须报错");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("回收站清单写入失败"),
        "文案要说明是清单写入失败：{msg}"
    );
    assert!(
        msg.contains("已回滚删除"),
        "回滚成功时要说清楚未删除任何文件：{msg}"
    );
    assert!(
        root.join("a.md").exists(),
        "文件必须被移回原位（FR-TRASH-12）"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.md")).expect("读回"),
        "# 甲\n"
    );
}
