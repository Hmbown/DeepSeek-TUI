import { useRef, useState, type KeyboardEvent } from "react";
import {
  Check,
  Copy,
  Edit3,
  ExternalLink,
  Folder,
  GitBranch,
  GitCommit,
  MoreHorizontal,
  Play,
  Terminal,
  Trash2,
  X,
} from "lucide-react";

import type { WorkspaceStatus } from "@/lib/runtime-api";

type TopBarProps = {
  workspace: WorkspaceStatus | null;
  threadTitle?: string;
  workspaceName?: string;
  onTitleChange?: (title: string) => void;
  onRun?: () => void;
  onFork?: () => void;
  onCompact?: () => void;
  onArchive?: () => void;
  onDelete?: () => void;
  onOpenInEditor?: () => void;
  onOpenInTerminal?: () => void;
  onOpenInFinder?: () => void;
  onCommit?: (message: string) => void;
};

export function TopBar({
  workspace,
  threadTitle,
  workspaceName,
  onTitleChange,
  onRun,
  onFork,
  onCompact,
  onArchive,
  onDelete,
  onOpenInEditor,
  onOpenInTerminal,
  onOpenInFinder,
  onCommit,
}: TopBarProps) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState("");
  const [showOverflow, setShowOverflow] = useState(false);
  const [showOpenMenu, setShowOpenMenu] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const openMenuRef = useRef<HTMLDivElement>(null);

  const startEditing = () => {
    if (!onTitleChange) return;
    setEditValue(threadTitle ?? "");
    setEditing(true);
  };

  const handleTitleDisplayKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!onTitleChange) return;
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    startEditing();
  };

  const saveTitle = () => {
    if (onTitleChange && editValue.trim()) {
      onTitleChange(editValue.trim());
    }
    setEditing(false);
  };

  const cancelEditing = () => {
    setEditing(false);
  };

  const hasStagedChanges = workspace?.git_repo && (workspace.staged ?? 0) > 0;

  return (
    <header className="topbar">
      <div className="topbar-title-area">
        {editing ? (
          <div className="topbar-title-edit">
            <input
              className="topbar-title-input"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveTitle();
                if (e.key === "Escape") cancelEditing();
              }}
              autoFocus
            />
            <button className="btn btn-ghost btn-sm" onClick={saveTitle} aria-label="Save title">
              <Check size={12} />
            </button>
            <button className="btn btn-ghost btn-sm" onClick={cancelEditing} aria-label="Cancel editing">
              <X size={12} />
            </button>
          </div>
        ) : (
          <div
            className="topbar-title-display"
            onClick={startEditing}
            onKeyDown={handleTitleDisplayKeyDown}
            role={onTitleChange ? "button" : undefined}
            tabIndex={onTitleChange ? 0 : undefined}
            aria-label={onTitleChange ? "Edit thread title" : undefined}
          >
            <h1 className="topbar-title-editable">{threadTitle ?? "New Thread"}</h1>
            {onTitleChange ? <Edit3 size={12} className="topbar-edit-icon" /> : null}
          </div>
        )}
        <div className="workspace-path">{workspaceName ?? workspace?.workspace ?? "No workspace"}</div>
        {workspace?.git_repo ? (
          <div className="workspace-git">
            <GitBranch size={13} />
            <span>{workspace.branch ?? "detached"}</span>
            <span>+{workspace.staged}</span>
            <span>~{workspace.unstaged}</span>
            <span>?{workspace.untracked}</span>
          </div>
        ) : null}
      </div>

      <div className="topbar-actions">
        {onRun ? (
          <button className="btn btn-primary btn-sm" onClick={onRun} aria-label="Run">
            <Play size={13} />
          </button>
        ) : null}

        {hasStagedChanges && onCommit ? (
          <button
            className="btn btn-ghost btn-sm commit-button"
            onClick={() => onCommit("Commit from Wagmii App")}
            aria-label="Commit staged changes"
          >
            <GitCommit size={13} />
            <span>Commit</span>
          </button>
        ) : null}

        {(onOpenInEditor || onOpenInTerminal || onOpenInFinder) ? (
          <div className="dropdown-wrap" ref={openMenuRef}>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => setShowOpenMenu((v) => !v)}
              aria-label="Open in..."
              aria-haspopup="true"
              aria-expanded={showOpenMenu}
            >
              <ExternalLink size={13} />
            </button>
            {showOpenMenu ? (
              <div className="dropdown-menu" role="menu">
                {onOpenInEditor ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onOpenInEditor(); setShowOpenMenu(false); }}>
                    <Copy size={12} />
                    <span>Open in VS Code</span>
                  </button>
                ) : null}
                {onOpenInTerminal ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onOpenInTerminal(); setShowOpenMenu(false); }}>
                    <Terminal size={12} />
                    <span>Open terminal</span>
                  </button>
                ) : null}
                {onOpenInFinder ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onOpenInFinder(); setShowOpenMenu(false); }}>
                    <Folder size={12} />
                    <span>Open in Finder</span>
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}

        {(onFork || onCompact || onArchive || onDelete) ? (
          <div className="dropdown-wrap" ref={overflowRef}>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => setShowOverflow((v) => !v)}
              aria-label="More actions"
              aria-haspopup="true"
              aria-expanded={showOverflow}
            >
              <MoreHorizontal size={14} />
            </button>
            {showOverflow ? (
              <div className="dropdown-menu overflow-menu" role="menu">
                {onFork ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onFork(); setShowOverflow(false); }}>
                    <Copy size={12} />
                    <span>Fork thread</span>
                  </button>
                ) : null}
                {onCompact ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onCompact(); setShowOverflow(false); }}>
                    <Terminal size={12} />
                    <span>Compact thread</span>
                  </button>
                ) : null}
                {onArchive ? (
                  <button className="dropdown-item" role="menuitem" onClick={() => { onArchive(); setShowOverflow(false); }}>
                    <Folder size={12} />
                    <span>Archive thread</span>
                  </button>
                ) : null}
                {onDelete ? (
                  <button className="dropdown-item dropdown-item-danger" role="menuitem" onClick={() => { onDelete(); setShowOverflow(false); }}>
                    <Trash2 size={12} />
                    <span>Delete thread</span>
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </header>
  );
}
