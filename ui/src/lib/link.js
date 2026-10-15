// 打开外部链接。
// Tauri 里的正式做法是 opener 插件（要在 Rust 侧注册权限），接宿主时换成它；
// 现在用 window.open，浏览器预览和无边框 WebView 都能用。
export function openExternal(url) {
  window.open(url, "_blank", "noopener,noreferrer");
}
