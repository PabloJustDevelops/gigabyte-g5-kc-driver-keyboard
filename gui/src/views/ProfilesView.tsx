import { useState, type CSSProperties } from "react";
import { Trash2, Play } from "lucide-react";
import { Button } from "../components/ui/button";
import { Section } from "../components/Section";
import { cn } from "../lib/cn";
import type { Backlight } from "../hooks/useBacklight";

export function ProfilesView({ kb }: { kb: Backlight }) {
  const { profiles, selected, dev } = kb;
  const locked = dev.effectRunning;
  const [name, setName] = useState("");

  const onSave = () => {
    kb.saveProfile(name);
    setName("");
  };

  return (
    <>
      <Section title="Save current" readout={`now: #${hex(dev)}`}>
        <label className="field-label" htmlFor="profile-name">
          Profile name
        </label>
        <div className="profile-actions">
          <input
            id="profile-name"
            className="name-input"
            maxLength={24}
            spellCheck={false}
            placeholder="e.g. gaming"
            value={name}
            disabled={locked}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSave();
            }}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={locked}
            onClick={onSave}
            className="whitespace-nowrap"
          >
            save current
          </Button>
        </div>
      </Section>

      <Section
        title={`Saved profiles (${profiles.length})`}
        readout="click apply to send"
      >
        {profiles.length === 0 && (
          <div className="readout">
            no saved profiles yet — pick a colour in Lighting and save it
          </div>
        )}
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {profiles.map((p) => {
            const isActive = selected === p.name;
            return (
              <div key={p.name} className={cn("profile-row", isActive && "active")}>
                <span
                  className="prow-dot"
                  style={{ "--pcol": "#" + p.color } as CSSProperties}
                />
                <div style={{ minWidth: 0 }}>
                  <div className="prow-name">{p.name}</div>
                  <div className="prow-meta">
                    #{p.color.toUpperCase()} · {p.brightness}% · {p.state}
                  </div>
                </div>
                <div className="prow-spacer" />
                <div className="prow-actions">
                  <Button
                    variant={isActive ? "default" : "outline"}
                    size="sm"
                    disabled={locked}
                    onClick={() => kb.applyProfile(p.name)}
                  >
                    <Play />
                    {isActive ? "applied" : "apply"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={locked}
                    onClick={() => kb.deleteProfile(p.name)}
                    aria-label={`Delete ${p.name}`}
                    className="hover:bg-destructive/15 hover:text-destructive"
                  >
                    <Trash2 />
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      </Section>
    </>
  );
}

const hex = (d: { red: number; green: number; blue: number }) =>
  [d.red, d.green, d.blue]
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("");
