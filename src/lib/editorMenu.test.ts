import { describe, expect, it } from "vitest";
import { buildOpenSubmenu } from "./editorMenu";

const detected = [
  { id: "vscode", label: "VS Code", command: "code" },
  { id: "trae", label: "Trae", command: "trae" },
];

describe("buildOpenSubmenu", () => {
  it("把检测结果转成子菜单项", () => {
    const items = buildOpenSubmenu(detected, () => {});
    expect(items).toHaveLength(2);
    expect(items[0].label).toBe("VS Code");
    expect(items[0].hint).toBe("code");
    expect(items[1].label).toBe("Trae");
  });

  it("点击项时回调对应编辑器 id", () => {
    const picked: string[] = [];
    const items = buildOpenSubmenu(detected, (id) => picked.push(id));
    items[1].onClick();
    expect(picked).toEqual(["trae"]);
  });

  it("没有可用编辑器时返回空数组（由菜单显示提示）", () => {
    expect(buildOpenSubmenu([], () => {})).toEqual([]);
  });

  it("编辑器被移除后结果同步减少（无缓存残留）", () => {
    // 模拟：先检测到两个，卸载 trae 后只检测到一个
    const afterUninstall = detected.filter((e) => e.id !== "trae");
    expect(buildOpenSubmenu(detected, () => {})).toHaveLength(2);
    expect(buildOpenSubmenu(afterUninstall, () => {})).toHaveLength(1);
    expect(buildOpenSubmenu(afterUninstall, () => {})[0].label).toBe("VS Code");
  });
});
