fn main() {
    // 宿主环境可能残留已废弃的 STATIC_VCRUNTIME 变量（tauri-build 2.7 起会打印
    // "STATIC_VCRUNTIME is deprecated" 警告）。静态链接 VC 运行时现在由
    // tauri.conf.json 的 build.windows.staticVCRuntime 决定（默认 true），
    // 这里在构建脚本内移除残留变量：行为不变，仅消除构建噪音。
    std::env::remove_var("STATIC_VCRUNTIME");
    tauri_build::build();
}
