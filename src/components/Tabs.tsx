import type { ReactNode } from "react";

type TabsProps<T extends string> = {
  items: readonly (readonly [T, string])[];
  value: T;
  onChange: (id: T) => void;
  /** Container class (default `tabs`); use when the row has its own layout. */
  className?: string;
  /** Trailing content in the same row (back button, inline form…). */
  children?: ReactNode;
};

/** Segmented tab row: active state + click wiring in one place. */
export default function Tabs<T extends string>({
  items,
  value,
  onChange,
  className = "tabs",
  children,
}: TabsProps<T>) {
  return (
    <div className={className}>
      {items.map(([id, label]) => (
        <button
          key={id}
          type="button"
          className={value === id ? "tab active" : "tab"}
          onClick={() => onChange(id)}
        >
          {label}
        </button>
      ))}
      {children}
    </div>
  );
}
