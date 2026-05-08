"use client";

import { useEffect, useRef, useState } from "react";

type Props = {
  chart: string;
  label?: string;
  fallback?: React.ReactNode;
};

export function MermaidDiagram({ chart, label, fallback }: Props) {
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const idRef = useRef(`mermaid-${Math.random().toString(36).slice(2, 9)}`);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mermaid = (await import("mermaid")).default;
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: "base",
          fontFamily: '"JetBrains Mono", ui-monospace, Menlo, monospace',
          flowchart: {
            curve: "basis",
            padding: 14,
            htmlLabels: false,
            useMaxWidth: true,
          },
          themeVariables: {
            background: "#ffffff",
            primaryColor: "#ffffff",
            primaryTextColor: "#0e0e10",
            primaryBorderColor: "#0e0e10",
            lineColor: "#4d6bfe",
            secondaryColor: "#e9eefe",
            tertiaryColor: "#f4f6fb",
            edgeLabelBackground: "#ffffff",
            clusterBkg: "#f4f6fb",
            clusterBorder: "#0e0e10",
            nodeBorder: "#0e0e10",
            mainBkg: "#ffffff",
          },
        });
        const { svg: rendered } = await mermaid.render(idRef.current, chart);
        if (!cancelled) setSvg(rendered);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [chart]);

  if (error) {
    return (
      <div className="mermaid-frame" role="img" aria-label={label}>
        <pre className="code-block text-[0.78rem]">{chart}</pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="mermaid-frame" role="img" aria-label={label} aria-busy="true">
        {fallback ?? (
          <pre className="code-block text-[0.78rem] opacity-70">{chart}</pre>
        )}
      </div>
    );
  }

  return (
    <div
      className="mermaid-frame"
      role="img"
      aria-label={label}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
