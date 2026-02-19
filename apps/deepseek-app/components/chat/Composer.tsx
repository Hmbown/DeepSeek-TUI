import { useRef, useState, type DragEvent } from "react";
import { ArrowUp, CornerDownLeft, FileText, Mic, Paperclip, Sparkles, X } from "lucide-react";

type AttachedFile = { name: string; path: string };

type ComposerProps = {
  value: string;
  onValueChange: (value: string) => void;
  onSend: () => void;
  onRetrySend: () => void;
  sending: boolean;
  selectedThreadId: string | null;
  activeTurnId: string | null;
  blockedSendReason?: string | null;
  canRetryBlockedSend?: boolean;
  mode: string;
  onModeChange: (mode: string) => void;
  model?: string;
  modelOptions?: string[];
  onModelChange?: (model: string) => void;
  modeOptions?: string[];
  attachedFiles?: AttachedFile[];
  onAttachedFilesChange?: (files: AttachedFile[]) => void;
  speechAvailable?: boolean;
  isListening?: boolean;
  onSpeechToggle?: () => void;
};

const MAX_ATTACHMENTS = 10;

export function Composer({
  value,
  onValueChange,
  onSend,
  onRetrySend,
  sending,
  selectedThreadId,
  activeTurnId,
  blockedSendReason,
  canRetryBlockedSend,
  mode,
  onModeChange,
  model,
  modelOptions,
  onModelChange,
  modeOptions,
  attachedFiles = [],
  onAttachedFilesChange,
  speechAvailable,
  isListening,
  onSpeechToggle,
}: ComposerProps) {
  const previousModeRef = useRef(mode === "plan" ? "agent" : mode);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  const togglePlanMode = () => {
    if (mode === "plan") {
      onModeChange(previousModeRef.current);
    } else {
      previousModeRef.current = mode;
      onModeChange("plan");
    }
  };

  const addFiles = (fileList: FileList | null) => {
    if (!fileList || !onAttachedFilesChange) return;
    const newFiles: AttachedFile[] = [];
    for (let i = 0; i < fileList.length; i++) {
      const file = fileList[i];
      if (attachedFiles.length + newFiles.length >= MAX_ATTACHMENTS) break;
      const filePath = ((file as File & { path?: string }).path ?? file.webkitRelativePath ?? file.name).trim() || file.name;
      const alreadyAttached =
        attachedFiles.some((f) => f.path === filePath || f.name === file.name) ||
        newFiles.some((f) => f.path === filePath || f.name === file.name);
      if (!alreadyAttached) {
        newFiles.push({ name: file.name, path: filePath });
      }
    }
    if (newFiles.length > 0) {
      onAttachedFilesChange([...attachedFiles, ...newFiles]);
    }
  };

  const removeFile = (name: string) => {
    if (!onAttachedFilesChange) return;
    onAttachedFilesChange(attachedFiles.filter((f) => f.name !== name));
  };

  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    addFiles(e.dataTransfer.files);
  };

  return (
    <div
      className={`composer ${isDragging ? "composer-dropzone is-dragging" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <label className="label" htmlFor="composer-input">
        Message
      </label>
      <div className="composer-input-row">
        {onAttachedFilesChange ? (
          <>
            <button
              className="btn btn-ghost btn-sm composer-attach-btn"
              onClick={() => fileInputRef.current?.click()}
              aria-label="Attach files"
              title="Attach files"
            >
              <Paperclip size={14} />
            </button>
            <input
              ref={fileInputRef}
              type="file"
              multiple
              style={{ display: "none" }}
              onChange={(e) => {
                addFiles(e.target.files);
                e.target.value = "";
              }}
            />
          </>
        ) : null}
        <textarea
          id="composer-input"
          rows={4}
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          placeholder="Type a prompt…"
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey) {
              event.preventDefault();
              if (!sending && value.trim() && !blockedSendReason) {
                onSend();
              }
            }

            if (event.key === "j" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              onValueChange(`${value}\n`);
            }

            if (event.key === "Enter" && event.altKey) {
              event.preventDefault();
              onValueChange(`${value}\n`);
            }
          }}
        />
      </div>

      {attachedFiles.length > 0 ? (
        <div className="attachment-chips">
          {attachedFiles.map((file) => (
            <span key={file.name} className="attachment-chip">
              <FileText size={11} />
              <span>{file.name}</span>
              <button
                className="attachment-chip-remove"
                onClick={() => removeFile(file.name)}
                aria-label={`Remove ${file.name}`}
              >
                <X size={10} />
              </button>
            </span>
          ))}
        </div>
      ) : null}

      <div className="composer-footer">
        <div className="composer-hint-row">
          <span>
            <Sparkles size={13} />
            <span>
              {selectedThreadId ? `Thread ${selectedThreadId.slice(0, 8)}` : "New thread"}
              {activeTurnId ? ` · active ${activeTurnId.slice(0, 8)}` : ""}
            </span>
          </span>
          <span>
            <CornerDownLeft size={13} />
            <span>Enter send · Shift+Enter newline · Ctrl/Cmd+J newline</span>
          </span>
          {blockedSendReason ? (
            <span className="inline-error" role="alert">
              <span>{blockedSendReason}</span>
            </span>
          ) : null}
        </div>

        <div className="composer-selectors">
          {modelOptions && onModelChange ? (
            <select
              className="composer-select"
              value={model}
              onChange={(e) => onModelChange(e.target.value)}
              aria-label="Model"
            >
              {modelOptions.map((opt) => (
                <option key={opt} value={opt}>{opt}</option>
              ))}
            </select>
          ) : null}
          {modeOptions ? (
            <select
              className="composer-select"
              value={mode}
              onChange={(e) => onModeChange(e.target.value)}
              aria-label="Mode"
            >
              {modeOptions.map((opt) => (
                <option key={opt} value={opt}>{opt}</option>
              ))}
            </select>
          ) : null}
        </div>

        <div className="inline-actions composer-actions">
          <button
            className={`btn btn-ghost btn-sm plan-toggle ${mode === "plan" ? "is-active" : ""}`}
            onClick={togglePlanMode}
            aria-pressed={mode === "plan"}
            title="Toggle plan mode"
          >
            <FileText size={13} />
            <span>Plan</span>
          </button>

          {speechAvailable && onSpeechToggle ? (
            <button
              className={`btn btn-ghost btn-sm mic-button ${isListening ? "is-recording" : ""}`}
              onClick={onSpeechToggle}
              aria-label={isListening ? "Stop recording" : "Start voice input"}
              title={isListening ? "Stop recording" : "Voice input"}
            >
              <Mic size={14} />
            </button>
          ) : null}

          {canRetryBlockedSend ? (
            <button className="btn btn-secondary" onClick={onRetrySend} aria-label="Retry send">
              Retry send
            </button>
          ) : null}
          <button
            className="btn btn-primary"
            disabled={sending || !value.trim() || !!blockedSendReason}
            onClick={onSend}
          >
            <ArrowUp size={14} />
            <span>{sending ? "Sending..." : "Send"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
