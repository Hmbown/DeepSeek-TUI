import { GitBranch, Shield, ShieldCheck, ShieldAlert, ShieldOff } from "lucide-react";

import type { WorkspaceStatus } from "@/lib/runtime-api";

type StatusBarProps = {
  mode: string;
  workspace: WorkspaceStatus | null;
  onInitGit: () => void;
};

type PermissionInfo = {
  label: string;
  className: string;
  Icon: typeof Shield;
};

function getPermissionInfo(mode: string): PermissionInfo {
  switch (mode) {
    case "yolo":
      return { label: "Full access", className: "permission-success", Icon: ShieldCheck };
    case "agent":
      return { label: "Agent mode", className: "permission-brand", Icon: Shield };
    case "plan":
      return { label: "Plan mode", className: "permission-warning", Icon: ShieldAlert };
    case "normal":
    default:
      return { label: "Approval required", className: "permission-neutral", Icon: ShieldOff };
  }
}

export function StatusBar({ mode, workspace, onInitGit }: StatusBarProps) {
  const { label, className, Icon } = getPermissionInfo(mode);
  const showGitInit = workspace && !workspace.git_repo;

  return (
    <div className="status-bar" role="status">
      <div className="status-bar-left">
        <span className={`permission-indicator ${className}`}>
          <Icon size={13} />
          <span>{label}</span>
        </span>
      </div>
      <div className="status-bar-right">
        {showGitInit ? (
          <button className="btn btn-ghost btn-sm" onClick={onInitGit}>
            <GitBranch size={13} />
            <span>Create git repository</span>
          </button>
        ) : null}
      </div>
    </div>
  );
}
