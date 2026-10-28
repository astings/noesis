import type { ReactNode } from 'react';

/** Bordered group of settings rows, divided by hairlines. */
export function SettingsGroup({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="max-w-[640px] space-y-2">
      {title && <h2 className="text-[13px] font-semibold text-foreground">{title}</h2>}
      <div className="rounded-lg border border-border divide-y divide-border">{children}</div>
    </section>
  );
}

/** One row: label and hint on the left, control on the right. Extra content (children below) spans the row. */
export function SettingRow({
  label,
  hint,
  control,
  children,
}: {
  label: ReactNode;
  hint?: ReactNode;
  control?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="px-3.5 py-3 space-y-3">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="text-sm font-medium text-foreground">{label}</div>
          {hint && <div className="text-xs text-muted-foreground">{hint}</div>}
        </div>
        {control && <div className="flex-none">{control}</div>}
      </div>
      {children}
    </div>
  );
}
