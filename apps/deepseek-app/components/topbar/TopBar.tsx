import { GitBranch, WandSparkles } from "lucide-react";

import type { WorkspaceStatus } from "@/lib/runtime-api";

type TopBarProps = {
  workspace: WorkspaceStatus | null;
  model: string;
  mode: string;
  modelOptions: string[];
  modeOptions: string[];
  onModelChange: (value: string) => void;
  onModeChange: (value: string) => void;
};

export function TopBar({
  workspace,
  model,
  mode,
  modelOptions,
  modeOptions,
  onModelChange,
  onModeChange,
}: TopBarProps) {
  return (
    <header className="topbar">
      <div className="workspace-summary">
        <div className="workspace-path">{workspace?.workspace ?? "No workspace selected"}</div>
        {workspace?.git_repo ? (
          <div className="workspace-git">
            <GitBranch size={13} />
            <span>{workspace.branch ?? "detached"}</span>
            <span>+{workspace.staged}</span>
            <span>~{workspace.unstaged}</span>
            <span>?{workspace.untracked}</span>
          </div>
        ) : (
          <div className="workspace-git muted">No git repository</div>
        )}
      </div>

      <div className="topbar-controls">
        <label>
          <span className="label">Model</span>
          <select value={model} onChange={(event) => onModelChange(event.target.value)}>
            {modelOptions.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>

        <label>
          <span className="label">Mode</span>
          <select value={mode} onChange={(event) => onModeChange(event.target.value)}>
            {modeOptions.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>

        <div className="mode-pill">
          <WandSparkles size={14} />
          <span>{mode}</span>
        </div>
      </div>
    </header>
  );
}
