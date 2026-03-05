# gw MCP Server

MCP server that exposes `gw` stacked branch operations as tools for Claude Code and other AI assistants.

## Setup

```bash
cd mcp-server
npm install
npm run build
```

## Add to Claude Code

Add to your `.claude/settings.json`:

```json
{
  "mcpServers": {
    "gw": {
      "command": "node",
      "args": ["/path/to/git-workflow/mcp-server/dist/index.js"]
    }
  }
}
```

## Available Tools

| Tool | Description |
| --- | --- |
| `gw_tree` | Show all stacks with branches and commits |
| `gw_tree_pr` | Show stacks with PR status |
| `gw_stack_create` | Create a new stack |
| `gw_stack_list` | List all stacks |
| `gw_branch_create` | Add a branch to the current stack |
| `gw_switch` | Switch to a tracked branch |
| `gw_rebase` | Propagate rebases to descendants |
| `gw_sync` | Fetch base, detect merges, rebase stack |
| `gw_push` | Push current branch |
