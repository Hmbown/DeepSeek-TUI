import { Link2Off, RefreshCcw, Wifi, WifiOff } from "lucide-react";

import type { ConnectionState } from "@/hooks/use-runtime-connection";

type ConnectionBannerProps = {
  state: ConnectionState;
  message: string;
  baseUrl: string;
  onRetryNow: () => void;
  onOpenSettings: () => void;
};

export function ConnectionBanner({
  state,
  message,
  baseUrl,
  onRetryNow,
  onOpenSettings,
}: ConnectionBannerProps) {
  if (state === "online") {
    return null;
  }

  return (
    <div className={`banner banner-${state}`} role="status" aria-live="polite">
      <div className="banner-main">
        {state === "checking" ? <Wifi size={16} /> : null}
        {state === "reconnecting" ? <Link2Off size={16} /> : null}
        {state === "offline" ? <WifiOff size={16} /> : null}
        <span>{message}</span>
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
