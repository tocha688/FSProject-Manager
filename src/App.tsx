import { getCurrentWindow } from "@tauri-apps/api/window";
import Library from "./views/Library";
import QuickSearch from "./views/QuickSearch";

// 同一个前端 bundle 服务两个窗口：quick 是全局快捷键唤起的快搜浮层，
// main 是档案柜主窗口。
export default function App() {
  const label = getCurrentWindow().label;
  return label === "quick" ? <QuickSearch /> : <Library />;
}
