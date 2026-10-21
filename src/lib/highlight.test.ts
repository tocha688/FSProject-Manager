import { describe, expect, it } from "vitest";
import { splitHighlight } from "./highlight";

describe("splitHighlight", () => {
  it("无词条时返回单段", () => {
    expect(splitHighlight("hello", [])).toEqual([{ text: "hello", hit: false }]);
    expect(splitHighlight("hello", ["", "  "])).toEqual([
      { text: "hello", hit: false },
    ]);
  });

  it("空文本返回单段", () => {
    expect(splitHighlight("", ["a"])).toEqual([{ text: "", hit: false }]);
  });

  it("大小写不敏感命中", () => {
    expect(splitHighlight("Resume-Parser", ["resume"])).toEqual([
      { text: "Resume", hit: true },
      { text: "-Parser", hit: false },
    ]);
  });

  it("中文子串命中", () => {
    expect(splitHighlight("简历解析爬虫", ["爬虫"])).toEqual([
      { text: "简历解析", hit: false },
      { text: "爬虫", hit: true },
    ]);
  });

  it("多词条都能高亮", () => {
    expect(splitHighlight("a-b-c", ["a", "c"])).toEqual([
      { text: "a", hit: true },
      { text: "-b-", hit: false },
      { text: "c", hit: true },
    ]);
  });

  it("相邻命中合并为一段", () => {
    expect(splitHighlight("abc", ["ab", "bc"])).toEqual([{ text: "abc", hit: true }]);
  });

  it("多次出现全部命中", () => {
    expect(splitHighlight("x.x.x", ["x"])).toEqual([
      { text: "x", hit: true },
      { text: ".", hit: false },
      { text: "x", hit: true },
      { text: ".", hit: false },
      { text: "x", hit: true },
    ]);
  });
});
