import { Section } from "../components/Section";

const PLANNED = [
  { name: "Balanced", desc: "stock fan curve · default power limits" },
  { name: "Performance", desc: "higher power limits · boost fan" },
  { name: "Quiet", desc: "low-noise fan curve" },
  { name: "Battery saver", desc: "reduced power draw on battery" },
];

export function PerformanceView() {
  return (
    <>
      <Section title="Performance" readout="planned">
        <p className="plan-note">
          These Gigabyte Control Center power &amp; fan profiles are not wired
          to any hardware yet: the kernel driver exposes only the keyboard
          backlight, and the EC path for CPU/fan control has not been
          reverse-engineered. Nothing on this page affects the machine.
        </p>
        <div className="plan-list">
          {PLANNED.map((p) => (
            <div key={p.name} className="plan-item">
              <div style={{ minWidth: 0 }}>
                <div className="plan-name">{p.name}</div>
                <div className="plan-desc">{p.desc}</div>
              </div>
              <span className="plan-status">needs EC wiring</span>
            </div>
          ))}
        </div>
      </Section>
    </>
  );
}
