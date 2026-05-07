# DeepSeek TUI — Plugin System

## Overview

Plugins extend DeepSeek TUI with custom tools, hooks, models, and middleware.
Plugins are JavaScript/TypeScript modules loaded at startup from
`~/.deepseek/plugins/` or `<workspace>/.deepseek/plugins/`.

## Plugin Manifest

Every plugin directory must contain a `plugin.json` manifest:

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "description": "What the plugin does",
  "author": "Your Name",
  "license": "MIT",
  "main": "index.js",
  "tools": ["my_custom_tool"],
  "hooks": ["PostToolUse"],
  "permissions": ["exec_shell"]
}
```

## Plugin API

Plugins receive a context object with:

- `registerTool(name, schema, handler)` — add a model-callable tool
- `registerHook(event, handler)` — react to lifecycle events
- `registerModelProvider(name, config)` — add a custom model provider
- `onShutdown(callback)` — cleanup on session end

## Example Plugin

```javascript
// ~/.deepseek/plugins/hello-world/index.js
module.exports = function(plugin) {
  plugin.registerTool('hello_world', {
    description: 'Say hello',
    inputSchema: {
      type: 'object',
      properties: {
        name: { type: 'string', description: 'Who to greet' }
      },
      required: ['name']
    }
  }, async (input, context) => {
    return `Hello, ${input.name}! Workspace: ${context.workspace}`;
  });

  plugin.registerHook('SessionStart', async (event) => {
    console.log(`Session started in ${event.cwd}`);
  });
};
```

## Security

Plugins run in a sandboxed Node.js VM with restricted filesystem access.
They cannot read files outside the workspace or make arbitrary network
requests. Permissions must be declared in `plugin.json` and are subject
to user approval.

## Publishing

Plugins can be published to npm with the `deepseek-plugin` keyword.
Users install with: `/plugin install npm:my-plugin`
