import { Command, MessageSquare, Plus, Settings2, Sparkles } from "lucide-react";

import type { Section } from "@/components/types";
import type { DesktopRunStateDetail } from "@/lib/run-state";

type LeftRailProps = {
  section: Section;
  onSectionChange: (section: Section) => void;
  onNewThread: () => void;
  onOpenPalette: () => void;
  runState: DesktopRunStateDetail;
};

function railHealthClass(state: DesktopRunStateDetail["state"]): string {
  switch (state) {
    case "online":
    case "completed":
      return "is-online";
    case "waiting-approval":
    case "running":
    case "reconnecting":
      return "is-reconnecting";
    case "failed":
      return "is-offline";
    case "checking":
    case "idle":
      return "is-checking";
    default:
      return "is-checking";
  }
}

export function LeftRail({
  section,
  onSectionChange,
  onNewThread,
  onOpenPalette,
  runState,
}: LeftRailProps) {
  return (
    <aside className="rail">
      <div className="rail-brand-wrap">
        <div className="rail-brand-icon" aria-hidden>
          <Sparkles size={18} />
        </div>
        <div>
          <div className="rail-brand">Assistant</div>
          <div className="rail-subbrand">AI Workspace</div>
        </div>
      </div>

      <div className="rail-group">
        <button className="btn btn-primary rail-btn" onClick={onNewThread}>
          <Plus size={16} />
          <span>New Thread</span>
        </button>

        <button
          className={`btn btn-ghost rail-btn ${section === "chat" ? "is-active" : ""}`}
          onClick={() => onSectionChange("chat")}
        >
          <MessageSquare size={16} />
          <span>Chat</span>
        </button>
        <button
          className={`btn btn-ghost rail-btn ${section === "automations" ? "is-active" : ""}`}
          onClick={() => onSectionChange("automations")}
        >
          <Sparkles size={16} />
          <span>Automations</span>
        </button>
        <button
          className={`btn btn-ghost rail-btn ${section === "skills" ? "is-active" : ""}`}
          onClick={() => onSectionChange("skills")}
        >
          <Command size={16} />
          <span>Skills & Apps</span>
        </button>
        <button
          className={`btn btn-ghost rail-btn ${section === "settings" ? "is-active" : ""}`}
          onClick={() => onSectionChange("settings")}
        >
          <Settings2 size={16} />
          <span>Settings</span>
        </button>
      </div>

      <div className="rail-footer">
        <button className="btn btn-ghost rail-btn" onClick={onOpenPalette}>
          <Command size={16} />
          <span>Command Palette</span>
          <kbd>Ctrl/Cmd+K</kbd>
        </button>
        <div className="rail-health">
          <span className={`health-dot ${railHealthClass(runState.state)}`} aria-hidden />
          <span>{runState.label}</span>
        </div>
      </div>
    </aside>
  );
}
