import type { Metadata } from "next";
import { Fraunces, IBM_Plex_Sans, JetBrains_Mono, Noto_Serif_SC } from "next/font/google";
import "./globals.css";

const display = Fraunces({
  subsets: ["latin"],
  weight: ["400", "500", "600", "700"],
  variable: "--font-display",
  display: "swap",
});

const body = IBM_Plex_Sans({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-body",
  display: "swap",
});

const mono = JetBrains_Mono({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-mono",
  display: "swap",
});

// Noto Serif SC is heavy; load only what we need for decorative anchors.
const cjk = Noto_Serif_SC({
  subsets: ["latin"],
  weight: ["400", "700"],
  variable: "--font-cjk",
  display: "swap",
  preload: false,
});

export const metadata: Metadata = {
  title: "DeepSeek TUI · 深度求索 终端",
  description:
    "Terminal-native coding agent built on DeepSeek V4. Open source. Community site for installation, docs, roadmap, and live activity from the Hmbown/deepseek-tui repo.",
  metadataBase: new URL("https://deepseek-tui.com"),
  openGraph: {
    title: "DeepSeek TUI",
    description: "Terminal-native coding agent built on DeepSeek V4.",
    url: "https://deepseek-tui.com",
    siteName: "DeepSeek TUI",
    type: "website",
  },
  twitter: { card: "summary_large_image" },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${display.variable} ${body.variable} ${mono.variable} ${cjk.variable}`}>
      <body>{children}</body>
    </html>
  );
}
