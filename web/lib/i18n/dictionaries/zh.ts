/**
 * zh-CN dictionary — written for native mainland-Chinese developers.
 * Full-width punctuation in CJK paragraphs. Natural phrasing, not calques from English.
 */
const zh = {
  nav: {
    links: [
      { href: "/zh/install", label: "安装", cn: "" },
      { href: "/zh/docs", label: "文档", cn: "" },
      { href: "/zh/feed", label: "动态", cn: "" },
      { href: "/zh/roadmap", label: "路线图", cn: "" },
      { href: "/zh/contribute", label: "参与贡献", cn: "" },
    ],
    edition: "第 … 期",
    online: "API · 在线",
    install: "立即安装 →",
    starGitHub: "★ GitHub",
  },
  footer: {
    cols: [
      {
        title: "产品",
        cn: "",
        items: [
          { label: "安装指南", href: "/zh/install" },
          { label: "使用文档", href: "/zh/docs" },
          { label: "路线图", href: "/zh/roadmap" },
          { label: "版本发布", href: "https://github.com/Hmbown/deepseek-tui/releases" },
        ],
      },
      {
        title: "社区",
        cn: "",
        items: [
          { label: "议题", href: "https://github.com/Hmbown/deepseek-tui/issues" },
          { label: "合并请求", href: "https://github.com/Hmbown/deepseek-tui/pulls" },
          { label: "讨论区", href: "https://github.com/Hmbown/deepseek-tui/discussions" },
          { label: "参与贡献", href: "/zh/contribute" },
        ],
      },
      {
        title: "资源",
        cn: "",
        items: [
          { label: "活动动态", href: "/zh/feed" },
          { label: "行为准则", href: "https://github.com/Hmbown/deepseek-tui/blob/main/CODE_OF_CONDUCT.md" },
          { label: "安全策略", href: "https://github.com/Hmbown/deepseek-tui/blob/main/SECURITY.md" },
          { label: "MIT 许可证", href: "https://github.com/Hmbown/deepseek-tui/blob/main/LICENSE" },
        ],
      },
    ],
    tagline:
      "基于 DeepSeek V4 的开源终端编程智能体。MIT 许可证。由一位维护者从得克萨斯独立维护。欢迎提交 Pull Request。",
    crafted: "用心制作 · Made with care",
    poweredBy: "本网站由 DeepSeek V4-Flash 协助维护",
    mirrors: "镜像源",
  },
  localeSwitch: { en: "EN", zh: "中文" },
};

export default zh;
