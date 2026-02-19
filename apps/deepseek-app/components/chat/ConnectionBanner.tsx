import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  Link2Off,
  RefreshCcw,
  Wifi,
  WifiOff,
} from "lucide-react";

import type { ConnectionState } from "@/hooks/use-runtime-connection";
import type { DesktopRunStateDetail } from "@/lib/run-state";

type ConnectionBannerProps = {
  state: ConnectionState;
  message: string;
  runState: DesktopRunStateDetail;
  baseUrl: string;
  onRetryNow: () => void;
  onOpenSettings: () => void;
};

function iconForRunState(runState: DesktopRunStateDetail["state"], connectionState: ConnectionState) {
  if (connectionState === "checking") {
    return <Wifi size={16} />;
  }
  if (connectionState === "offline") {
    return <WifiOff size={16} />;
  }
  if (connectionState === "reconnecting") {
    return <Link2Off size={16} />;
  }
  if (runState === "waiting-approval") {
    return <Clock3 size={16} />;
  }
  if (runState === "failed") {
    return <AlertTriangle size={16} />;
  }
  if (runState === "completed") {
    return <CheckCircle2 size={16} />;
  }
  return <Wifi size={16} />;
}

export function ConnectionBanner({
  state,
  message,
  runState,
  baseUrl,
  onRetryNow,
  onOpenSettings,
}: ConnectionBannerProps) {
  if (state === "online" && (runState.state === "online" || runState.state === "idle")) {
    return null;
  }

  return (
    <div className={`banner banner-${runState.state}`} role="status" aria-live="polite">
      <div className="banner-main">
        {iconForRunState(runState.state, state)}
        <div className="banner-copy">
          <span className={`status-chip status-${runState.state}`}>{runState.label}</span>
          <span>{runState.reason || message}</span>
        </div>
      </div>
      <div className="banner-actions">
        <button className="btn btn-ghost btn-sm" onClick={onRetryNow}>
          <RefreshCcw size={14} />
          <span>Retry now</span>
        </button>
        <button className="btn btn-ghost btn-sm" onClick={onOpenSettings}>
          Runtime: {baseUrl}
        </button>
      </div>
    </div>
  );
}
