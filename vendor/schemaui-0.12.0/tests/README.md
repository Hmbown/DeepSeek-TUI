# SchemaUI 测试套件

本目录包含 SchemaUI 项目的所有测试相关文件，包括端到端测试、Web UI
测试和测试数据。

## 📁 目录结构

```
tests/
├── e2e/                          # 端到端测试
│   └── e2e-test-suite.js       # Puppeteer E2E 测试套件
├── web-ui/                       # Web UI 专用测试
│   ├── web-ui-automated-tests.js # 自动化UI测试脚本
│   └── automated-ui-tests.html   # 浏览器内测试界面
├── schemas/                      # 测试用 Schema 文件
│   └── test-comprehensive.schema.json
├── test-package.json            # 测试依赖配置
└── README.md                    # 本文档
```

## 🚀 快速开始

### 安装依赖

```bash
cd tests
npm install --package-lock=false --prefix .
```

### 运行测试

#### 1. 端到端测试 (E2E)

```bash
# 在可视化模式下运行
npm run test:e2e --prefix tests

# 在无头模式下运行（CI/CD）
npm run test:e2e:headless --prefix tests
```

#### 2. Web UI 测试

##### 方法一：使用自动化脚本

```bash
npm run test:web-ui --prefix tests
```

##### 方法二：使用浏览器界面

1. 启动 SchemaUI Web 服务器：

```bash
./target/debug/schemaui web -s examples/complex.schema.json --port 5175
```

2. 在浏览器中打开测试界面：

```bash
open tests/web-ui/automated-ui-tests.html
```

3. 点击界面中的测试按钮执行相应测试

## 📋 测试覆盖范围

### E2E 测试

- ✅ 数字输入字段（简单和复杂类型）
- ✅ OneOf/AnyOf 变体切换
- ✅ 数组 CRUD 操作
- ✅ 深层嵌套结构
- ✅ 表单验证

### Web UI 测试

- ✅ 数字输入更新验证
- ✅ 文本输入处理
- ✅ 变体选择器功能
- ✅ JSON 输出验证
- ✅ UI 状态同步

## 🐛 已知问题和修复

### 数字输入问题

**问题**：在 oneOf/anyOf 对象内的数字字段无法正确更新 JSON 值。

**状态**：✅ 已修复

**测试路径**：`/e/e1/e2/e3/e4/logic`

**相关文档**：参见 `docs/fixes/FINAL_FIX_SUMMARY.md`

## 🔧 测试配置

### Puppeteer 配置

测试使用 Puppeteer 进行浏览器自动化。默认配置：

- 浏览器：Chromium
- 模式：可视化（可通过环境变量切换）
- 超时：30 秒

### 测试数据

测试使用 `schemas/test-comprehensive.schema.json` 作为主要测试数据，包含：

- 复杂嵌套结构
- OneOf/AnyOf 组合
- 各种数据类型
- 边界情况

## 📝 添加新测试

### 添加 E2E 测试

在 `e2e/e2e-test-suite.js` 中添加新的测试用例：

```javascript
const newTestCase = {
  name: "测试名称",
  path: "/path/to/field",
  variant: "variant-name", // 可选
  field: "fieldName",
  value: expectedValue,
};

// 添加到相应的测试数组中
numberInputTests.push(newTestCase);
```

### 添加 Web UI 测试

在 `web-ui/web-ui-automated-tests.js` 中添加新的测试方法：

```javascript
async testNewFeature() {
  // 测试逻辑
  return {
    success: true/false,
    details: {}
  };
}
```

## 🤝 贡献指南

1. 所有新功能必须包含相应的测试
2. 修复 Bug 时必须添加回归测试
3. 测试必须能在 CI 环境中运行
4. 保持测试的独立性和可重复性

## 📚 相关文档

- [项目主文档](../README.md)
- [修复报告](../docs/fixes/)
- [API 文档](../docs/en/)

## 🏷️ 版本历史

- v1.0.0 - 初始测试套件
  - 基础 E2E 测试
  - Web UI 自动化测试
  - 数字输入问题修复验证
