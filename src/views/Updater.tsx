import { useCallback, useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import * as api from "../lib/api";

/** 便携版更新信息（从 GitHub Release 的 latest.json 读取）。 */
const LATEST_JSON_URL =
  "https://github.com/tocha688/FSProject-Manager/releases/latest/download/latest.json";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "uptodate" }
  | { kind: "downloading"; progress: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

function isNewer(remote: string, current: string): boolean {
  const a = remote.split(".").map(Number);
  const b = current.split(".").map(Number);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

/**
 * 测速并选出最快的镜像前缀。
 * 直连在 800ms 内就不折腾镜像（多一跳代理反而慢，也少一个信任点）。
 */
async function pickMirrorPrefix(): Promise<string> {
  try {
    const results = await api.speedTestMirrors(LATEST_JSON_URL);
    const direct = results.find((r) => r.id === "direct");
    if (direct?.ok && direct.elapsedMs <= 800) return "";
    const fastest = results.filter((r) => r.ok).sort((a, b) => a.elapsedMs - b.elapsedMs)[0];
    if (!fastest || fastest.id === "direct") return "";
    const all = await api.listMirrors();
    return all.find((m) => m.id === fastest.id)?.prefix ?? "";
  } catch {
    return ""; // 测速失败就直连
  }
}

export function useUpdater() {
  const [status, setStatus] = useState<UpdateStatus>({ kind: "idle" });
  const [update, setUpdate] = useState<Update | null>(null);
  // undefined = 还在检测中；null = 检测失败（按非便携版处理）
  const [portable, setPortable] = useState<api.PortableInfo | null | undefined>(undefined);
  const [portableUrl, setPortableUrl] = useState<string>("");

  useEffect(() => {
    api
      .portableInfo()
      .then(setPortable)
      .catch(() => setPortable(null));
  }, []);

  const checkForUpdate = useCallback(
    async (silent = false) => {
      setStatus({ kind: "checking" });
      try {
        const current = await api.getAppVersion();

        // 运行形态还没确定时必须先等它确定，否则便携版会被误当成安装版，
        // 调用内置 updater 直接失败（表现为"检查更新失败"）。
        let info = portable;
        if (info === undefined) {
          info = await api.portableInfo().catch(() => null);
          setPortable(info);
        }

        // 便携版：读 latest.json，找对应平台的便携包
        if (info?.isPortable) {
          // 必须由后端发起请求：WebView 里 fetch 外部 HTTPS 会被跨域策略拦，
          // 报 "Failed to fetch"，用户看到的就是"检查更新失败"。
          const prefix = await pickMirrorPrefix();
          const manifest = await api.fetchUpdateManifest(LATEST_JSON_URL, prefix);
          if (!isNewer(manifest.version, current)) {
            setStatus({ kind: "uptodate" });
            return;
          }
          if (!manifest.portableUrl) {
            throw new Error(
              `更新清单里没有当前平台（${info.platformKey}）的便携包`,
            );
          }
          setPortableUrl(manifest.portableUrl);
          setStatus({
            kind: "available",
            version: manifest.version,
            notes: manifest.notes ?? "",
          });
          return;
        }

        // 安装版：走 Tauri 内置 updater
        const u = await check();
        if (u) {
          setUpdate(u);
          setStatus({ kind: "available", version: u.version, notes: u.body ?? "" });
        } else {
          setStatus({ kind: "uptodate" });
        }
      } catch (e) {
        if (silent) setStatus({ kind: "idle" });
        else setStatus({ kind: "error", message: String(e) });
      }
    },
    [portable],
  );

  const downloadAndInstall = useCallback(async () => {
    setStatus({ kind: "downloading", progress: 0 });

    // 便携版：下载 zip → 解压 → 替换并重启
    if (portable?.isPortable) {
      try {
        const version = (status as { version?: string }).version ?? "";
        const r = await api.downloadPortableUpdate(portableUrl, version);
        setStatus({ kind: "ready" });
        // 给用户一点时间看到提示，然后应用替换
        window.setTimeout(async () => {
          try {
            await api.applyPortableUpdate(r.stagingDir);
          } catch (e) {
            setStatus({ kind: "error", message: String(e) });
          }
        }, 1200);
      } catch (e) {
        setStatus({ kind: "error", message: String(e) });
      }
      return;
    }

    // 安装版
    if (!update) return;
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const pct = total > 0 ? Math.round((downloaded / total) * 100) : 0;
          setStatus({ kind: "downloading", progress: pct });
        }
      });
      setStatus({ kind: "ready" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    }
  }, [update, portable, portableUrl, status]);

  const restart = useCallback(async () => {
    await relaunch();
  }, []);

  useEffect(() => {
    const t = window.setTimeout(() => void checkForUpdate(true), 3000);
    return () => window.clearTimeout(t);
  }, [checkForUpdate]);

  return { status, checkForUpdate, downloadAndInstall, restart, portable };
}

/** 设置页里的更新区块。 */
export function UpdateSection() {
  const { status, checkForUpdate, downloadAndInstall, restart, portable } = useUpdater();
  const [version, setVersion] = useState("");

  useEffect(() => {
    api.getAppVersion().then(setVersion).catch(() => {});
  }, []);

  return (
    <section>
      <h2>软件更新</h2>
      <p className="dim">
        当前版本：{version || "…"}
        {portable?.isPortable ? "（便携版，更新会替换当前目录内的程序文件）" : "（安装版）"}
        。启动时自动检查，也可手动检查。
      </p>

      <div className="row wrap">
        <button onClick={() => void checkForUpdate(false)} disabled={status.kind === "checking"}>
          {status.kind === "checking" ? "检查中…" : "检查更新"}
        </button>

        {status.kind === "available" && (
          <button className="primary" onClick={() => void downloadAndInstall()}>
            下载并安装 {status.version}
          </button>
        )}
        {status.kind === "ready" && !portable?.isPortable && (
          <button className="primary" onClick={() => void restart()}>
            重启以完成更新
          </button>
        )}
      </div>

      {status.kind === "available" && status.notes && (
        <pre className="update-notes">{status.notes}</pre>
      )}
      {status.kind === "uptodate" && <p className="dim small">已是最新版本。</p>}
      {status.kind === "downloading" && (
        <div className="update-progress">
          <div className="bar" style={{ width: `${status.progress}%` }} />
          <span>{status.progress}%</span>
        </div>
      )}
      {status.kind === "ready" && (
        <p className="dim small">
          {portable?.isPortable
            ? "更新包已就绪，正在替换程序并重启…"
            : "下载完成，重启后生效。"}
        </p>
      )}
      {status.kind === "error" && <p className="err small">检查失败：{status.message}</p>}
    </section>
  );
}
