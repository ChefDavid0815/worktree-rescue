import clsx from "clsx";
import {
  createContext,
  forwardRef,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, Check, Copy, Loader2, X } from "lucide-react";
import { useI18n } from "../lib/i18n";
import { CommandError } from "../lib/ipc";

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "quiet";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
  trailing?: ReactNode;
  loading?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon, trailing, loading, className, children, disabled, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      className={clsx("btn", `btn-${variant}`, `btn-${size}`, loading && "is-loading", className)}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? <Loader2 className="btn-icon spin" size={15} aria-hidden /> : icon}
      {children && <span className="btn-label">{children}</span>}
      {trailing}
    </button>
  );
});

export function IconButton({
  label,
  icon,
  className,
  active,
  ...rest
}: { label: string; icon: ReactNode; active?: boolean } & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      className={clsx("icon-btn", active && "is-active", className)}
      aria-label={label}
      title={label}
      {...rest}
    >
      {icon}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Badges, dots, rings
// ---------------------------------------------------------------------------

export type Tone = "neutral" | "cyan" | "amber" | "red" | "violet";

export function Badge({
  tone = "neutral",
  children,
  className,
  title,
  icon,
}: {
  tone?: Tone;
  children: ReactNode;
  className?: string;
  title?: string;
  icon?: ReactNode;
}) {
  return (
    <span className={clsx("badge", `tone-${tone}`, className)} title={title}>
      {icon}
      {children}
    </span>
  );
}

export function Dot({ tone = "neutral", pulse }: { tone?: Tone; pulse?: boolean }) {
  return <span className={clsx("dot", `tone-${tone}`, pulse && "pulse")} aria-hidden />;
}

export type RingState = "protected" | "stale" | "unprotected" | "missing" | "unknown" | "paused" | "clean";

/** Protection ring echoing the app mark: a closed ring means captured. */
export function ProtectionRing({ state, size = 28, title }: { state: RingState; size?: number; title?: string }) {
  const r = 10;
  const c = 2 * Math.PI * r;
  const dash: Record<RingState, string> = {
    protected: `${c * 0.82} ${c * 0.18}`,
    clean: `${c * 0.82} ${c * 0.18}`,
    stale: `${c * 0.5} ${c * 0.5}`,
    unprotected: `${c * 0.12} ${c * 0.13}`,
    missing: `${c * 0.2} ${c * 0.8}`,
    unknown: `${c * 0.06} ${c * 0.14}`,
    paused: `${c * 0.82} ${c * 0.18}`,
  };
  return (
    <svg
      className={clsx("ring", `ring-${state}`)}
      width={size}
      height={size}
      viewBox="0 0 28 28"
      role="img"
      aria-label={title}
    >
      {title && <title>{title}</title>}
      <circle cx="14" cy="14" r={r} className="ring-track" />
      <circle
        cx="14"
        cy="14"
        r={r}
        className="ring-arc"
        strokeDasharray={dash[state]}
        strokeDashoffset={c * 0.66}
      />
      <circle cx="14" cy="14" r="4" className="ring-core" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className={clsx("toggle-row", disabled && "is-disabled")}>
      <div className="toggle-text">
        <label htmlFor={id} className="toggle-label">
          {label}
        </label>
        {description && <p className="toggle-desc">{description}</p>}
      </div>
      <button
        id={id}
        role="switch"
        aria-checked={checked}
        className={clsx("switch", checked && "is-on")}
        onClick={() => onChange(!checked)}
        disabled={disabled}
      >
        <span className="switch-thumb" />
      </button>
    </div>
  );
}

export function Checkbox({
  checked,
  indeterminate,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  indeterminate?: boolean;
  onChange: (v: boolean) => void;
  label?: string;
  disabled?: boolean;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!indeterminate;
  }, [indeterminate]);
  return (
    <input
      ref={ref}
      type="checkbox"
      className="checkbox"
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
      aria-label={label}
      disabled={disabled}
    />
  );
}

export const TextInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }>(
  function TextInput({ className, mono, ...rest }, ref) {
    return <input ref={ref} className={clsx("input", mono && "mono", className)} spellCheck={false} {...rest} />;
  },
);

export function Field({
  label,
  hint,
  children,
  htmlFor,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  htmlFor?: string;
}) {
  return (
    <div className="field">
      <label className="field-label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {hint && <p className="field-hint">{hint}</p>}
    </div>
  );
}

export function Segmented<V extends string>({
  options,
  value,
  onChange,
  label,
  size = "md",
}: {
  options: { value: V; label: ReactNode; title?: string }[];
  value: V;
  onChange: (v: V) => void;
  label: string;
  size?: "sm" | "md";
}) {
  return (
    <div className={clsx("segmented", `segmented-${size}`)} role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={o.value === value}
          className={clsx("segment", o.value === value && "is-active")}
          onClick={() => onChange(o.value)}
          title={o.title}
        >
          <span className="segment-label">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/** Path rendered in mono with middle truncation; full path in the title. */
export function PathText({ path, max = 64, className }: { path: string; max?: number; className?: string }) {
  let shown = path;
  if (path.length > max) {
    const head = Math.ceil((max - 1) * 0.38);
    const tail = max - 1 - head;
    shown = `${path.slice(0, head)}…${path.slice(path.length - tail)}`;
  }
  return (
    <span className={clsx("path mono selectable", className)} title={path}>
      {shown}
    </span>
  );
}

export function Hash({ value, n = 8, copy = true }: { value: string | null | undefined; n?: number; copy?: boolean }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  if (!value) return <span className="mono muted">—</span>;
  return (
    <span className="hash mono selectable" title={value}>
      {value.slice(0, n)}
      {copy && (
        <button
          className="hash-copy"
          aria-label={t("common.copy")}
          title={copied ? t("common.copied") : t("common.copy")}
          onClick={(e) => {
            e.stopPropagation();
            void navigator.clipboard?.writeText(value).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1200);
            });
          }}
        >
          {copied ? <Check size={11} /> : <Copy size={11} />}
        </button>
      )}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

// ---------------------------------------------------------------------------
// Feedback: skeleton, empty, error, progress
// ---------------------------------------------------------------------------

export function Skeleton({ w = "100%", h = 14, className }: { w?: number | string; h?: number; className?: string }) {
  return <span className={clsx("skeleton", className)} style={{ width: w, height: h }} aria-hidden />;
}

export function SkeletonRows({ rows = 6 }: { rows?: number }) {
  return (
    <div className="skeleton-rows" aria-busy="true">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="skeleton-row">
          <Skeleton w={18} h={18} />
          <Skeleton w={`${40 + ((i * 17) % 45)}%`} />
          <Skeleton w={60} />
        </div>
      ))}
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  body,
  action,
  compact,
}: {
  icon?: ReactNode;
  title: ReactNode;
  body?: ReactNode;
  action?: ReactNode;
  compact?: boolean;
}) {
  return (
    <div className={clsx("empty", compact && "empty-compact")}>
      {icon && <div className="empty-icon">{icon}</div>}
      <h3 className="empty-title">{title}</h3>
      {body && <p className="empty-body">{body}</p>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

export function ErrorBlock({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const { t, td } = useI18n();
  const code = error instanceof CommandError ? error.code : "other";
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="error-block" role="alert">
      <AlertTriangle size={16} aria-hidden />
      <div>
        <p className="error-title">{td(`error.${code}`, t("error.other"))}</p>
        <p className="error-detail mono selectable">{message}</p>
      </div>
      {onRetry && (
        <Button size="sm" variant="ghost" onClick={onRetry}>
          {t("common.retry")}
        </Button>
      )}
    </div>
  );
}

export function ProgressBar({ value, indeterminate, tone = "cyan" }: { value?: number; indeterminate?: boolean; tone?: Tone }) {
  return (
    <div
      className={clsx("progress", `tone-${tone}`, indeterminate && "is-indeterminate")}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round((value ?? 0) * 100)}
    >
      <span className="progress-fill" style={{ width: indeterminate ? undefined : `${Math.min(1, value ?? 0) * 100}%` }} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog (native <dialog> for focus management)
// ---------------------------------------------------------------------------

export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  width = 520,
  tone,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  tone?: Tone;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const { t } = useI18n();
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) {
      if (typeof d.showModal === "function") d.showModal();
      else d.setAttribute("open", "");
    } else if (!open && d.open) {
      if (typeof d.close === "function") d.close();
      else d.removeAttribute("open");
    }
  }, [open]);
  return (
    <dialog
      ref={ref}
      className={clsx("dialog", tone && `dialog-${tone}`)}
      style={{ width }}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      {open && (
        <div className="dialog-inner">
          <header className="dialog-header">
            <h2 className="dialog-title">{title}</h2>
            <IconButton label={t("common.close")} icon={<X size={16} />} onClick={onClose} />
          </header>
          <div className="dialog-body">{children}</div>
          {footer && <footer className="dialog-footer">{footer}</footer>}
        </div>
      )}
    </dialog>
  );
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

interface Toast {
  id: number;
  tone: Tone;
  title: string;
  body?: string;
}

const ToastContext = createContext<(t: Omit<Toast, "id">) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((t: Omit<Toast, "id">) => {
    const id = Date.now() + Math.random();
    setToasts((s) => [...s.slice(-3), { ...t, id }]);
    setTimeout(() => setToasts((s) => s.filter((x) => x.id !== id)), t.tone === "red" ? 9000 : 5000);
  }, []);
  return (
    <ToastContext.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        <AnimatePresence initial={false}>
          {toasts.map((t) => (
            <motion.div
              key={t.id}
              className={clsx("toast", `tone-${t.tone}`)}
              initial={{ y: 12, scale: 0.98 }}
              animate={{ y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, transition: { duration: 0.15 } }}
              transition={{ type: "spring", stiffness: 420, damping: 34 }}
            >
              <span className={clsx("toast-bar", `tone-${t.tone}`)} aria-hidden />
              <div>
                <p className="toast-title">{t.title}</p>
                {t.body && <p className="toast-body">{t.body}</p>}
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
}

export const useToast = () => useContext(ToastContext);

// ---------------------------------------------------------------------------
// Popover anchored with position: fixed (escapes overflow containers)
// ---------------------------------------------------------------------------

export function Popover({
  anchor,
  open,
  onClose,
  children,
  width = 320,
  align = "start",
}: {
  anchor: HTMLElement | null;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  align?: "start" | "end";
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  useEffect(() => {
    if (!open || !anchor) return;
    const r = anchor.getBoundingClientRect();
    setPos({
      top: r.bottom + 6,
      left: align === "start" ? r.left : Math.max(8, r.right - width),
    });
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node) && !anchor.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, anchor, onClose, align, width]);
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          ref={ref}
          className="popover"
          style={{ top: pos.top, left: pos.left, width }}
          initial={{ y: -4, scale: 0.985 }}
          animate={{ y: 0, scale: 1 }}
          exit={{ opacity: 0, y: -2, transition: { duration: 0.12 } }}
          transition={{ type: "spring", stiffness: 520, damping: 38 }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
