import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";

interface Props {
  onUnlocked: () => void;
}

/** 锁屏界面：密码输入 + 错误提示。 */
export default function LockScreen({ onUnlocked }: Props) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const submit = useCallback(async () => {
    if (!password || busy) return;
    setBusy(true);
    setError("");
    try {
      const ok = await api.unlock(password);
      if (ok) {
        setPassword("");
        onUnlocked();
      } else {
        setError("密码不正确");
        setPassword("");
        inputRef.current?.focus();
      }
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  }, [password, busy, onUnlocked]);

  return (
    <div className="lockscreen">
      <div className="lock-card">
        <div className="lock-icon">🔒</div>
        <h2>已锁定</h2>
        <p className="dim">输入密码以查看项目</p>
        <input
          ref={inputRef}
          type="password"
          className="lock-input"
          placeholder="密码"
          value={password}
          autoFocus
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
          }}
        />
        {error && <div className="lock-error">{error}</div>}
        <button className="primary lock-btn" onClick={submit} disabled={!password || busy}>
          {busy ? "验证中…" : "解锁"}
        </button>
      </div>
    </div>
  );
}
