import { KB_ROWS } from "../lib/keyboard";
import { cn } from "../lib/cn";

interface Props {
  lit: boolean;
  running: boolean;
  /** effect mode that is approximating on the preview */
  fx: "breathe" | "cycle" | null;
  /** hero scale — the keyboard leads the page (overview / lighting) */
  hero?: boolean;
}

export function KeyboardPreview({ lit, running, fx, hero }: Props) {
  return (
    <section className={cn("stage card", hero && "kb-hero")}>
      <div className="keyboard" aria-hidden="true">
        <div className="kb-rows">
          {KB_ROWS.map((row, i) => (
            <div className="kb-row" key={i}>
              {row.map((k, j) => {
                if (k.gap) {
                  return (
                    <span
                      key={j}
                      className="key gap"
                      style={{ flexGrow: k.u ?? 1 }}
                    />
                  );
                }
                return (
                  <span
                    key={j}
                    className={cn("key", lit && "lit", k.space && "space")}
                    style={{ flexGrow: k.u ?? 1 }}
                  >
                    {k.l}
                  </span>
                );
              })}
            </div>
          ))}
        </div>
      </div>
      <p className="caption">
        Preview
        {running && fx && <span className="fx-note"> · approximating live effect</span>}
      </p>
    </section>
  );
}
