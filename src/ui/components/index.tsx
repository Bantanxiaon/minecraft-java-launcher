import { useEffect, useRef } from "react";
import type {
  ButtonHTMLAttributes,
  ReactNode,
  RefObject,
} from "react";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "quiet" | "danger" | "danger-quiet";
  size?: "sm" | "md" | "lg";
};

export function Button({
  variant = "default",
  size = "md",
  className = "",
  type = "button",
  ...rest
}: ButtonProps) {
  const variantClass =
    variant === "primary"
      ? "btn-primary"
      : variant === "quiet"
        ? "btn-quiet"
        : variant === "danger"
          ? "btn-danger"
          : variant === "danger-quiet"
            ? "btn-danger-quiet"
            : "";
  const sizeClass = size === "sm" ? "btn-sm" : size === "lg" ? "btn-lg" : "";
  return (
    <button
      type={type}
      className={`btn ${variantClass} ${sizeClass} ${className}`.trim()}
      {...rest}
    />
  );
}

export function IconButton({
  label,
  children,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  label: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`btn btn-icon ${className}`.trim()}
      {...rest}
    >
      {children}
    </button>
  );
}

export function Badge({
  tone = "default",
  children,
}: {
  tone?: "default" | "success" | "warning" | "danger" | "info";
  children: ReactNode;
}) {
  const toneClass =
    tone === "success"
      ? "badge-success"
      : tone === "warning"
        ? "badge-warn"
        : tone === "danger"
          ? "badge-danger"
          : tone === "info"
            ? "badge-info"
            : "";
  return <span className={`badge ${toneClass}`.trim()}>{children}</span>;
}

export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
  label,
}: {
  tabs: Array<{ id: T; label: string }>;
  value: T;
  onChange: (id: T) => void;
  label: string;
}) {
  return (
    <div className="ui3-tabs" role="tablist" aria-label={label}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={value === tab.id}
          className={value === tab.id ? "active" : ""}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

export function Progress({
  value,
  indeterminate = false,
  className = "",
}: {
  value?: number;
  indeterminate?: boolean;
  className?: string;
}) {
  const percent =
    value === undefined ? undefined : Math.max(0, Math.min(100, value));
  return (
    <div
      className={`progress-track ${className}`}
      role="progressbar"
      aria-valuenow={percent === undefined ? undefined : percent}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div
        className={`progress-fill ${indeterminate ? "indeterminate" : ""}`}
        style={percent === undefined ? undefined : { width: `${percent}%` }}
      />
    </div>
  );
}

export function Spinner({ size = 18 }: { size?: number }) {
  return (
    <span
      className="ui3-spinner"
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon}
      <h2>{title}</h2>
      {description ? <p>{description}</p> : null}
      {action}
    </div>
  );
}

export function ErrorState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="error-state" role="alert">
      <h2>{title}</h2>
      {description ? <p>{description}</p> : null}
      {action}
    </div>
  );
}

export function Skeleton({
  width = "100%",
  height = 14,
  className = "",
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) {
  return (
    <div
      className={`skeleton ${className}`}
      style={{ width, height }}
      aria-hidden="true"
    />
  );
}

export function Tooltip({
  text,
  children,
}: {
  text: string;
  children: ReactNode;
}) {
  return (
    <span className="tooltip-wrap" data-tooltip={text}>
      {children}
    </span>
  );
}

export function Dialog({
  open,
  title,
  onClose,
  children,
  actions,
  initialFocusRef,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  actions?: ReactNode;
  initialFocusRef?: RefObject<HTMLButtonElement | null>;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    const timer = window.setTimeout(() => {
      if (initialFocusRef?.current) {
        initialFocusRef.current.focus();
      } else {
        dialogRef.current?.focus();
      }
    }, 0);
    return () => {
      window.clearTimeout(timer);
      previousFocus.current?.focus?.();
    };
  }, [open, initialFocusRef]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        ref={dialogRef}
      >
        <div className="dialog-head">
          <h2>{title}</h2>
          <IconButton label="关闭" onClick={onClose}>
            ×
          </IconButton>
        </div>
        {children}
        {actions ? <div className="dialog-actions">{actions}</div> : null}
      </div>
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="switch"
      onClick={() => onChange(!checked)}
    />
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: Array<{ id: T; label: string }>;
  value: T;
  onChange: (id: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          role="radio"
          aria-checked={value === option.id}
          className={value === option.id ? "active" : ""}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
