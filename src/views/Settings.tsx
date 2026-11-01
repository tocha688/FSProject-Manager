import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../lib/api";
import type { DataInfo, SettingsView } from "../lib/types";

export default function Settings({ onBack }: { onBack: () => void }) {
  const [s, setS] = useState<SettingsView | null>(null);
  const [info, setInfo] = useState<DataInfo | null>(null);
  const [toast, setToast] = useState("");
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    api.getSettings().then(setS).catch((e) => setToast(String(e)));
    api.getDataInfo().then(setInfo).catch(() => {});
  }, []);

  if (!s) {
    return (
      <div className="app">
        <p className="dim">加载中…</p>
      </div>
    );
  }

  const notify = (m: string) => {
    setToast(m);
    window.setTimeout(() => setToast((t) => (t === m ? "" : t)), 3500);
  };

  const patch = (u: Partial<SettingsView>) => setS({ ...s, ...u });
  const patchAi = (u: Partial<SettingsView["ai"]>) => setS({ ...s, ai: { ...s.ai, ...u } });

  // 后端权威判定：配置指纹与当前配置一致才算已验证
  const verified = s.aiVerified;

  const addRoot = async () => {
    const path = await openDialog({ directory: true, multiple: false });
    if (!path) return;
    if (s.roots.some((r) => r.path === path)) {
      notify("该目录已在列表中");
      return;
    }
    patch({ roots: [...s.roots, { path, depth: 5 }] });
  };

  const save = async () => {
    try {
      const { aiVerified: _omit, ...payload } = s;
      await api.saveSettings(payload);
      // 回读：后端可能因校验未通过而把 enabled 强制关掉
      const fresh = await api.getSettings();
      setS(fresh);
      notify("设置已保存");
    } catch (e) {
      notify(String(e));
    }
  };

  const test = async () => {
    setTesting(true);
    try {
      const { aiVerified: _omit, ...payload } = s;
      await api.saveSettings(payload);
      const msg = await api.testAi();
      const fresh = await api.getSettings();
      setS(fresh);
      notify(msg);
    } catch (e) {
      notify(String(e));
    }
    setTesting(false);
  };

  return (
    <div className="app">
      <header>
        <h1>设置</h1>
        <div className="actions">
          <button onClick={onBack}>返回</button>
          <button className="primary" onClick={save}>
            保存
          </button>
        </div>
      </header>

      <div className="form">
        <section>
          <h2>扫描目录</h2>
          <p className="dim">
            添加后自动递归扫描其中的项目；每个目录可单独设置扫描深度（默认向下 5 层）。
          </p>
          {s.roots.map((r, i) => (
            <div className="row" key={r.path}>
              <span className="path grow">{r.path}</span>
              <label className="inline">
                深度
                <input
                  type="number"
                  min={1}
                  max={12}
                  value={r.depth}
                  onChange={(e) => {
                    const roots = [...s.roots];
                    roots[i] = { ...r, depth: Number(e.target.value) || 1 };
                    patch({ roots });
                  }}
                />
              </label>
              <button onClick={() => patch({ roots: s.roots.filter((_, j) => j !== i) })}>
                移除
              </button>
            </div>
          ))}
          <button onClick={addRoot}>＋ 添加目录</button>
        </section>

        <section>
          <h2>AI 分析</h2>
          <p className="dim">
            支持任何 OpenAI 兼容接口（智谱 GLM / OpenAI / DeepSeek 等）与本地 Ollama。
            未开启时项目仍可入库并用关键词搜索。
          </p>
          <div className="row wrap">
            <label className="inline">
              服务
              <select
                value={s.ai.provider}
                onChange={(e) => patchAi({ provider: e.target.value, verifiedFingerprint: "" })}
              >
                <option value="openai-compatible">云端 API（OpenAI 兼容）</option>
                <option value="ollama">本地 Ollama</option>
              </select>
            </label>
            <label className="inline">
              API 地址
              <input
                value={s.ai.baseUrl}
                onChange={(e) => patchAi({ baseUrl: e.target.value, verifiedFingerprint: "" })}
                placeholder="https://open.bigmodel.cn/api/paas/v4"
              />
            </label>
            <label className="inline">
              模型
              <input
                value={s.ai.model}
                onChange={(e) => patchAi({ model: e.target.value, verifiedFingerprint: "" })}
                placeholder="glm-4-flash"
              />
            </label>
            <label className="inline">
              API Key
              <input
                type="password"
                value={s.ai.apiKey}
                onChange={(e) => patchAi({ apiKey: e.target.value, verifiedFingerprint: "" })}
                placeholder="Ollama 留空"
              />
            </label>
            <label className="inline">
              并发数
              <input
                type="number"
                min={1}
                max={8}
                value={s.ai.concurrency}
                onChange={(e) => patchAi({ concurrency: Number(e.target.value) || 1 })}
                style={{ width: "4.5em" }}
              />
            </label>
            <button onClick={test} disabled={testing}>
              {testing ? "测试中…" : "测试连接"}
            </button>
          </div>

          <div className={`switch-row ${verified ? "ok" : "warn"}`}>
            <label className="switch">
              <input
                type="checkbox"
                checked={s.ai.enabled}
                disabled={!verified}
                onChange={(e) => patchAi({ enabled: e.target.checked })}
              />
              <span>启用 AI 分析</span>
            </label>
            <span className="switch-hint">
              {verified
                ? s.ai.enabled
                  ? "已启用：扫描后自动分析，也可手动触发"
                  : "配置已验证，可开启"
                : "请先「测试连接」通过后才能开启"}
            </span>
          </div>
        </section>

        <section>
          <h2>快捷键与其他</h2>
          <div className="row wrap">
            <label className="inline">
              全局快捷键
              <input
                value={s.shortcut}
                onChange={(e) => patch({ shortcut: e.target.value })}
                placeholder="alt+space"
              />
            </label>
            <label className="inline">
              编辑器命令
              <input
                value={s.editorCmd}
                onChange={(e) => patch({ editorCmd: e.target.value })}
                placeholder="code"
              />
            </label>
            <label className="inline check">
              <input
                type="checkbox"
                checked={s.autostart}
                onChange={(e) => patch({ autostart: e.target.checked })}
              />
              开机自启（保持托盘常驻）
            </label>
          </div>
          {info && (
            <p className="dim small">
              数据目录（便携式，跟随软件运行目录）：{info.dbPath}
            </p>
          )}
        </section>
      </div>

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
