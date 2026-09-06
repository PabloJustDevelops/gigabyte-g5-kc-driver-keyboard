import type { ReactNode } from "react";

interface Props {
  title: string;
  readout?: string;
  children: ReactNode;
  className?: string;
}

export function Section({ title, readout, children, className }: Props) {
  return (
    <section className={`card block ${className ?? ""}`.trim()}>
      <div className="head-row">
        <h2>{title}</h2>
        {readout && <span className="readout">{readout}</span>}
      </div>
      {children}
    </section>
  );
}
