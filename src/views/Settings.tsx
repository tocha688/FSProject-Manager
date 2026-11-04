import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../lib/api";
import type { DataInfo, SettingsView } from "../lib/types";

export default function Settings({ onBack }: { onBack: () => void }) {
  const [s, setS] = useState<SettingsView | null>(null);
  const [info, setInfo] = useState<DataInfo | null>(null);
  const [toast, setToast] = useState("");
  const [testing, setTesting] = useState(false);
  // 密码管理
  const [oldPwd, setOldPwd] = useState("");
  const [newPwd, setNewPwd] = useState("");
  const [confirmPwd, setConfirmPwd] = useState("");
  const [pwdBusy, setPwdBusy] = useState(false);
  const [hasPassword, setHasPassword] = useState(false);

  useEffect(() => {
    api.getSettings().then(setS).catch((e) => setToast(String(e)));
    api.getDataInfo().then(setInfo).catch(() => {});
    api.getLockState().then((l) => setHasPassword(l.hasPassword)).catch(() => {});
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

  const savePassword = async () => {
    if (newPwd !== confirmPwd) {
      notify("两次输入的新密码不一致");
      return;
    }
    if (newPwd.length < 4) {
      notify("密码至少 4 个字符");
      return;
    }
    setPwdBusy(true);
    try {
      await api.setPassword(hasPassword ? oldPwd : null, newPwd, s.security?.idleLockMinutes);
      setHasPassword(true);
      setOldPwd("");
      setNewPwd("");
      setConfirmPwd("");
      notify("密码已设置");
    } catch (e) {
      notify(String(e));
    }
    setPwdBusy(false);
  };

  const clearPassword = async () => {
    if (!window.confirm("清除密码后将不再需要解锁，确定吗？")) return;
    setPwdBusy(true);
    try {
      await api.setPassword(oldPwd, "", s.security?.idleLockMinutes);
      setHasPassword(false);
      setOldPwd("");
      notify("密码已清除，锁屏已关闭");
    } catch (e) {
      notify(String(e));
    }
    setPwdBusy(false);
  };

  const lockNow = async () => {
    try {
      await api.lockNow();
      notify("已锁定，返回后可看到锁屏");
    } catch (e) {
      notify(String(e));
    }
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
      const wantEnabled = s.ai.enabled;
      await api.saveSettings(payload);
      // 回读：后端可能因校验未通过而把 enabled 强制关掉
      const fresh = await api.getSettings();
      setS(fresh);
      if (wantEnabled && !fresh.ai.enabled) {
        notify("已保存，但 AI 分析未能启用：配置未通过连接测试，请先点「测试连接」");
      } else {
        notify("设置已保存");
      }
    } catch (e) {
      notify(String(e));
    }
  };

  const test = async () => {
    setTesting(true);
    notify("正在测试连接…");
    try {
      const { aiVerified: _omit, ...payload } = s;
      // 先落盘当前配置，测试才会用最新的地址/模型/Key
      await api.saveSettings(payload);
      const r = await api.testAi();
      // 后端已写入指纹，回读即可解锁开关（不必再手动保存一次）
      const fresh = await api.getSettings();
      setS({ ...fresh, aiVerified: true });
      notify(r.message);
    } catch (e) {
      notify(`测试失败：${String(e)}`);
    } finally {
      setTesting(false);
    }
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
        <section>
          <h2>密码保护</h2>
          <p className="dim">
            设置密码后，启动应用、长时间无操作、以及窗口退到后台超时，都需要输入密码才能查看项目。
          </p>
          <p className="dim small">
            当前状态：{s.security?.passwordHash ? "已设置密码" : "未设置密码"}
          </p>

          <div className="row wrap">
            {(s.security?.passwordHash || hasPassword) && (
              <label className="inline">
                原密码
                <input
                  type="password"
                  value={oldPwd}
                  onChange={(e) => setOldPwd(e.target.value)}
                  placeholder="修改或清除时必填"
                />
              </label>
            )}
            <label className="inline">
              {hasPassword ? "新密码" : "设置密码"}
              <input
                type="password"
                value={newPwd}
                onChange={(e) => setNewPwd(e.target.value)}
                placeholder="至少 4 个字符"
              />
            </label>
            <label className="inline">
              确认
              <input
                type="password"
                value={confirmPwd}
                onChange={(e) => setConfirmPwd(e.target.value)}
              />
            </label>
            <label className="inline">
              空闲自动锁定
              <input
                type="number"
                min={0}
                max={120}
                value={s.security?.idleLockMinutes ?? 1}
                onChange={(e) =>
                  patch({
                    security: {
                      ...(s.security ?? { passwordHash: "" }),
                      idleLockMinutes: Number(e.target.value) || 0,
                    },
                  })
                }
                style={{ width: "4.5em" }}
              />
              分钟（0 = 不自动锁）
            </label>
          </div>

          <div className="row wrap">
            <button className="primary" onClick={savePassword} disabled={pwdBusy}>
              {hasPassword ? "更新密码" : "设置密码"}
            </button>
            {hasPassword && (
              <button className="danger" onClick={clearPassword} disabled={pwdBusy}>
                清除密码（关闭锁屏）
              </button>
            )}
            {hasPassword && <button onClick={lockNow}>立即锁定</button>}
          </div>
        </section>

      </div>

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
