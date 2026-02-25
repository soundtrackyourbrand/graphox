# Neovim Configuration for `Graphox`

To use the `Graphox` LSP with Neovim, you can configure it either using `nvim-lspconfig` or manually with `vim.lsp.start`.

## Quick Start

If you have `@graphox/cli` installed via npm/pnpm in your project, use this simple setup:

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.graphox then
  configs.graphox = {
    default_config = {
      cmd = { 'pnpm', 'exec', 'graphox', 'lsp' },
      filetypes = { 'graphql', 'typescript', 'typescriptreact', 'javascript', 'javascriptreact' },
      root_dir = lspconfig.util.root_pattern('graphox.yml', 'graphox.yaml', 'package.json'),
      settings = {},
    },
  }
end

lspconfig.graphox.setup({})
```

## Configuration Options

### Option 1: Using npm/pnpm Package (Recommended for Projects)

This is the easiest setup if you're working on a project that already uses `@graphox/cli`:

```lua
-- Using pnpm
cmd = { 'pnpm', 'exec', 'graphox', 'lsp' }

-- Using npm
cmd = { 'npm', 'exec', '--', 'graphox', 'lsp' }

-- Using yarn
cmd = { 'yarn', 'exec', 'graphox', 'lsp' }
```

**Benefits:**
- Always uses the version from your project's `package.json`
- No need to install globally or manage PATH
- Works in monorepos with different versions per project

### Option 2: Using System PATH

If you have `Graphox` installed globally:

```lua
cmd = { 'graphox', 'lsp' }
```

### Option 3: Using Local Build

For developing `Graphox` itself:

```lua
cmd = { '/path/to/graphox/target/release/graphox', 'lsp' }
-- or for debug build
cmd = { '/path/to/graphox/target/debug/graphox', 'lsp' }
```

## Full Setup with Custom Commands

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.graphox then
  configs.graphox = {
    default_config = {
      -- Choose one of these cmd options:
      cmd = { 'pnpm', 'exec', 'graphox', 'lsp' },  -- npm/pnpm package
      -- cmd = { 'graphox', 'lsp' },               -- system PATH
      -- cmd = { '/full/path/to/graphox', 'lsp' }, -- local build
      filetypes = { 'graphql', 'typescript', 'typescriptreact', 'javascript', 'javascriptreact' },
      root_dir = lspconfig.util.root_pattern('graphox.yml', 'graphox.yaml', 'package.json'),
      settings = {},
    },
  }
end

lspconfig.graphox.setup({
  on_attach = function(client, bufnr)
    local function execute_command(command)
      vim.lsp.buf.execute_command({
        command = command,
        arguments = {},
      })
    end

    vim.api.nvim_buf_create_user_command(bufnr, 'GraphQLCodegen', function()
      execute_command('graphox.runCodegen')
    end, { desc = 'Run GraphQL code generation' })

    vim.api.nvim_buf_create_user_command(bufnr, 'GraphQLClearCache', function()
      execute_command('graphox.clearCache')
    end, { desc = 'Clear GraphQL LSP cache and reload schemas' })

    vim.api.nvim_buf_create_user_command(bufnr, 'GraphQLRestart', function()
      vim.lsp.buf.stop_client(client.id)
      vim.cmd('Edit')
      vim.lsp.buf.start_client(client)
    end, { desc = 'Restart GraphQL LSP server' })

    vim.keymap.set('n', '<leader>gc', ':GraphQLCodegen<CR>', { buffer = bufnr, desc = 'GraphQL Codegen' })
    vim.keymap.set('n', '<leader>gr', ':GraphQLClearCache<CR>', { buffer = bufnr, desc = 'GraphQL Reload' })
    vim.keymap.set('n', '<leader>gs', ':GraphQLRestart<CR>', { buffer = bufnr, desc = 'GraphQL Restart Server' })
  end,
})
```

## Supported Custom Actions

The server currently supports two primary custom workspace commands:

- **`graphox.clearCache`**: Manually triggers the TypeScript type generation for all operations and fragments in the workspace.
- **`graphox.clearCache`**: Clears all parsed schemas and re-validates all open documents. This is useful if you've made manual changes to a schema file that wasn't picked up by the file watcher.

## Automatic Code Actions

The LSP also provides standard **Code Actions** (Quickfixes) that you can access via `vim.lsp.buf.code_action()`. These include:

- **Remove unused fragment**: Offered when a fragment is defined but not used.
- **Remove unused variable**: Offered when a variable is declared in an operation but not used in the selection set.
- **Extract Fragment**: Offered when a selection of fields can be extracted into a new fragment.

## Monorepo Support

For monorepos with multiple projects, you can use `lspconfig` with a custom root pattern:

```lua
lspconfig.graphox.setup({
  cmd = { 'pnpm', 'exec', 'graphox', 'lsp' },
  filetypes = { 'graphql', 'typescript', 'typescriptreact', 'javascript', 'javascriptreact' },
  root_dir = function(fname)
    return lspconfig.util.root_pattern(
      'graphox.yaml',
      'package.json',
      '.git'
    )(fname) or vim.fn.getcwd()
  end,
})
```

## Troubleshooting

- **Server won't start**: Check that `Graphox` is in your PATH or use the full path to the binary.
- **Version mismatch**: If using npm/pnpm, ensure the version in `package.json` is correct.
- **Path issues**: Use absolute paths for local builds: `cmd = { '/absolute/path/to/binary', 'lsp' }`
- **File watching**: The LSP automatically registers file watchers. No additional configuration needed.

## Additional Setup Tips

- **File Watching**: The LSP automatically registers file watchers for your schema files (as defined in your `graphql.config.yml`). You don't need additional Neovim configuration for schema reloading on save.
- **Semantic Tokens**: `Graphox` provides high-fidelity semantic highlighting for GraphQL blocks inside template literals. Ensure your Neovim version supports `LspTokenUpdate` (0.9+) for the best experience.
