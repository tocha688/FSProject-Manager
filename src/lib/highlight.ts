export interface Segment {
  text: string;
  hit: boolean;
}

/** 把文本按命中词切分成片段，用于搜索结果高亮。大小写不敏感，支持中文。 */
export function splitHighlight(text: string, terms: string[]): Segment[] {
  const clean = terms.map((t) => t.toLowerCase()).filter((t) => t.length > 0);
  if (clean.length === 0 || text.length === 0) return [{ text, hit: false }];
  const lower = text.toLowerCase();
  const marks = new Array<boolean>(text.length).fill(false);
  for (const t of clean) {
    let i = lower.indexOf(t);
    while (i !== -1) {
      for (let j = i; j < i + t.length; j++) marks[j] = true;
      i = lower.indexOf(t, i + t.length);
    }
  }
  const out: Segment[] = [];
  let cur = "";
  let curHit = marks[0];
  for (let i = 0; i < text.length; i++) {
    if (marks[i] === curHit) {
      cur += text[i];
    } else {
      out.push({ text: cur, hit: curHit });
      cur = text[i];
      curHit = marks[i];
    }
  }
  if (cur) out.push({ text: cur, hit: curHit });
  return out;
}
