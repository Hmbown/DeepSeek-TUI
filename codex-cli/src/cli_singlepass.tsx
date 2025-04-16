import type { AppConfig } from "./utils/config";

import { SinglePassApp } from "./components/singlepass-cli-app";
import { render } from "ink";
import React from "react";

export async function runSinglePass({
  originalPrompt,
  config,
  rootPath,
}: {
  originalPrompt?: string;
  config: AppConfig;
  rootPath: string;
}): Promise<void> {
  return new Promise((resolve) => {
    // In full context mode we want to capture Ctrl+C ourselves so we can use it
    // to interrupt long‑running requests without force‑quitting the whole
    // process. Ink exits automatically when it detects Ctrl+C unless
    // `exitOnCtrlC` is disabled via the render‑options, so make sure to turn it
    // off here. All other keyboard handling (including optionally exiting when
    // the user presses Ctrl+C while at the main prompt) is implemented inside
    // `SinglePassApp`.

    render(
      <SinglePassApp
        originalPrompt={originalPrompt}
        config={config}
        rootPath={rootPath}
        onExit={() => resolve()}
      />,
      {
        exitOnCtrlC: false,
      },
    );
  });
}

export default {};
