import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

export type ThemeMode = "dark" | "light" | "system";

type ThemeContextValue = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
};

const ThemeContext = createContext<ThemeContextValue>({
  mode: "dark",
  setMode: () => {},
});

export function ThemeProvider({
  children,
  mode: controlledMode,
  onModeChange,
}: {
  children: ReactNode;
  mode?: ThemeMode;
  onModeChange?: (mode: ThemeMode) => void;
}) {
  const [internalMode, setModeState] = useState<ThemeMode>(() => {
    try {
      const saved = localStorage.getItem("sh-ui3-theme");
      if (saved === "light" || saved === "system") return saved;
    } catch {
      // storage unavailable
    }
    return "dark";
  });
  const mode = controlledMode ?? internalMode;

  useEffect(() => {
    document.documentElement.dataset.theme = mode;
    try {
      localStorage.setItem("sh-ui3-theme", mode);
    } catch {
      // storage unavailable
    }
  }, [mode]);

  const setMode = (next: ThemeMode) => {
    if (onModeChange) {
      onModeChange(next);
    } else {
      setModeState(next);
    }
  };

  return (
    <ThemeContext.Provider value={{ mode, setMode }}>
      {children}
    </ThemeContext.Provider>
  );
}

export const useTheme = () => useContext(ThemeContext);

export type ToastKind = "success" | "error" | "info";
export type ToastItem = {
  id: number;
  kind: ToastKind;
  text: string;
};

type ToastContextValue = {
  toasts: ToastItem[];
  push: (kind: ToastKind, text: string) => void;
  dismiss: (id: number) => void;
};

const ToastContext = createContext<ToastContextValue>({
  toasts: [],
  push: () => {},
  dismiss: () => {},
});

let toastSeq = 1;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const dismiss = (id: number) => {
    setToasts((existing) => existing.filter((item) => item.id !== id));
  };

  const push = (kind: ToastKind, text: string) => {
    const id = toastSeq++;
    setToasts((existing) => [...existing.slice(-4), { id, kind, text }]);
    window.setTimeout(() => dismiss(id), 4200);
  };

  return (
    <ToastContext.Provider value={{ toasts, push, dismiss }}>
      {children}
    </ToastContext.Provider>
  );
}

export const useToasts = () => useContext(ToastContext);

export function ToastStack() {
  const { toasts, dismiss } = useToasts();
  if (!toasts.length) return null;
  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`toast toast-${toast.kind}`}
          role={toast.kind === "error" ? "alert" : "status"}
        >
          <span>{toast.text}</span>
          <button
            type="button"
            className="btn-icon"
            aria-label="关闭提示"
            onClick={() => dismiss(toast.id)}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
