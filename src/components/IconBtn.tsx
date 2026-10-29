import type { ReactNode, Ref } from "react";

type IconBtnProps = {
  icon: ReactNode;
  /** Tooltip; also the accessible name unless `label` overrides it. */
  title: string;
  label?: string;
  onClick?: () => void;
  disabled?: boolean;
  /** Toggle state (`aria-pressed`). */
  pressed?: boolean;
  /** Disclosure state (`aria-expanded`). */
  expanded?: boolean;
  /** Work in progress: adds `.spin`, `aria-busy` and disables the button. */
  busy?: boolean;
  /** Extra classes appended after `icon-btn` (e.g. `square danger`). */
  className?: string;
  buttonRef?: Ref<HTMLButtonElement>;
};

/** Shared icon-only button: `.icon-btn` chrome, tooltip + accessible name. */
export default function IconBtn({
  icon,
  title,
  label,
  onClick,
  disabled,
  pressed,
  expanded,
  busy,
  className,
  buttonRef,
}: IconBtnProps) {
  return (
    <button
      ref={buttonRef}
      type="button"
      className={["icon-btn", className, busy ? "spin" : ""]
        .filter(Boolean)
        .join(" ")}
      onClick={onClick}
      disabled={disabled || busy}
      title={title}
      aria-label={label ?? title}
      aria-pressed={pressed}
      aria-expanded={expanded}
      aria-busy={busy || undefined}
    >
      {icon}
    </button>
  );
}
