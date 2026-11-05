import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * 集成测试：验证 latest-json.mjs 生成的平台键与产物架构一致。
 *
 * 背景（真实 bug）：macOS 的 .app.tar.gz 文件名不含架构，
 * 早期实现会把它归到 darwin-x86_64，导致 Apple Silicon 用户
 * 查更新时匹配不到 darwin-aarch64，永远收不到更新。
 */
const SCRIPT = join(process.cwd(), "scripts/latest-json.mjs");

function buildManifest(files: string[]): any {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-test-"));
  try {
    for (const f of files) {
      writeFileSync(join(dir, f), f.endsWith(".sig") ? "fakesig" : "x");
    }
    execFileSync(process.execPath, [SCRIPT, dir, "owner/repo", "v1.2.3"], {
      stdio: "pipe",
    });
    return JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("latest-json 平台键推断", () => {
  it("macOS arm64：.app.tar.gz 应归到 darwin-aarch64", () => {
    const m = buildManifest([
      "ProjectFinder_1.2.3_x64-setup.exe",
      "ProjectFinder_1.2.3_x64-setup.exe.sig",
      "ProjectFinder_1.2.3_aarch64.dmg",
      "ProjectFinder.app.tar.gz",
      "ProjectFinder.app.tar.gz.sig",
      "ProjectFinder_1.2.3_amd64.AppImage",
      "ProjectFinder_1.2.3_amd64.AppImage.sig",
    ]);
    expect(m.platforms["darwin-aarch64"]).toBeDefined();
    expect(m.platforms["darwin-aarch64"].url).toContain("ProjectFinder.app.tar.gz");
    expect(m.platforms["darwin-x86_64"]).toBeUndefined();
  });

  it("macOS x86_64：dmg 为 x64 时应归到 darwin-x86_64", () => {
    const m = buildManifest([
      "ProjectFinder_1.2.3_x64.dmg",
      "ProjectFinder.app.tar.gz",
      "ProjectFinder.app.tar.gz.sig",
      "ProjectFinder_1.2.3_x64-setup.exe",
      "ProjectFinder_1.2.3_x64-setup.exe.sig",
    ]);
    expect(m.platforms["darwin-x86_64"]).toBeDefined();
    expect(m.platforms["darwin-aarch64"]).toBeUndefined();
  });

  it("三个平台都能生成", () => {
    const m = buildManifest([
      "ProjectFinder_1.2.3_x64-setup.exe",
      "ProjectFinder_1.2.3_x64-setup.exe.sig",
      "ProjectFinder_1.2.3_aarch64.dmg",
      "ProjectFinder.app.tar.gz",
      "ProjectFinder.app.tar.gz.sig",
      "ProjectFinder_1.2.3_amd64.AppImage",
      "ProjectFinder_1.2.3_amd64.AppImage.sig",
    ]);
    expect(Object.keys(m.platforms).sort()).toEqual([
      "darwin-aarch64",
      "linux-x86_64",
      "windows-x86_64",
    ]);
  });

  it("便携版单独归类，不混进 platforms", () => {
    const m = buildManifest([
      "ProjectFinder_1.2.3_x64-setup.exe",
      "ProjectFinder_1.2.3_x64-setup.exe.sig",
      "ProjectFinder_1.2.3_windows_x64_portable.zip",
      "ProjectFinder_1.2.3_linux_x64_portable.tar.gz",
      "ProjectFinder_1.2.3_macos_aarch64_portable.tar.gz",
    ]);
    expect(m.portable["windows-x86_64"]).toBeDefined();
    expect(m.portable["linux-x86_64"]).toBeDefined();
    expect(m.portable["darwin-aarch64"]).toBeDefined();
    // 便携版不应出现在安装版清单里
    expect(JSON.stringify(m.platforms)).not.toContain("portable");
  });

  it("URL 必须纯 ASCII（GitHub Release 会剥离中文导致 404）", () => {
    const m = buildManifest([
      "ProjectFinder_1.2.3_x64-setup.exe",
      "ProjectFinder_1.2.3_x64-setup.exe.sig",
    ]);
    for (const v of Object.values<any>(m.platforms)) {
      // 不应出现百分号编码的中文
      expect(v.url).not.toMatch(/%E[0-9A-F]{2}/i);
    }
  });

  it("没有带签名的包时应失败而不是生成空清单", () => {
    expect(() =>
      buildManifest(["ProjectFinder_1.2.3_x64-setup.exe"]), // 无 .sig
    ).toThrow();
  });
});
