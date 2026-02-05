# Neovim Configuration for `graphql-rust`

To use the `graphql-rust` LSP with Neovim, you can configure it either using `nvim-lspconfig` or manually with `vim.lsp.start`.

## Configuration with `nvim-lspconfig`

You can add the following to your Neovim configuration (usually `init.lua`). This setup registers the server for GraphQL, TypeScript, and JavaScript files, and adds custom user commands to trigger the LSP actions.

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

-- 1. Register the custom server configuration if not already present
if not configs.graphql_rust then
  configs.graphql_rust = {
    default_config = {
      cmd = { 'graphql-rust', 'lsp' },
      filetypes = { 'graphql', 'typescript', 'typescriptreact', 'javascript', 'javascriptreact' },
      root_dir = lspconfig.util.root_pattern('graphql.yml', 'graphql.yaml'),
      settings = {},
    },
  }
end

-- 2. Setup the server and register custom commands
lspconfig.graphql_rust.setup({
  on_attach = function(client, bufnr)
    -- Helper function to execute LSP commands
    local function execute_command(command)
      vim.lsp.buf.execute_command({
        command = command,
        arguments = {},
      })
    end

    -- Register Neovim commands for custom LSP actions
    vim.api.nvim_buf_create_user_command(bufnr, 'GraphQLCodegen', function()
      execute_command('graphql.runCodegen')
    end, { desc = 'Run GraphQL code generation' })

    vim.api.nvim_buf_create_user_command(bufnr, 'GraphQLClearCache', function()
      execute_command('graphql.clearCache')
    end, { desc = 'Clear GraphQL LSP cache and reload schemas' })

    -- Optional: Keymaps for the custom actions
    vim.keymap.set('n', '<leader>gc', ':GraphQLCodegen<CR>', { buffer = bufnr, desc = 'GraphQL Codegen' })
    vim.keymap.set('n', '<leader>gr', ':GraphQLClearCache<CR>', { buffer = bufnr, desc = 'GraphQL Reload' })
  end,
})
```

## Supported Custom Actions

The server currently supports two primary custom workspace commands:

- **`graphql.runCodegen`**: Manually triggers the TypeScript type generation for all operations and fragments in the workspace.
- **`graphql.clearCache`**: Clears all parsed schemas and re-validates all open documents. This is useful if you've made manual changes to a schema file that wasn't picked up by the file watcher.

## Automatic Code Actions

The LSP also provides standard **Code Actions** (Quickfixes) that you can access via `vim.lsp.buf.code_action()`. These include:
- **Remove unused fragment**: Offered when a fragment is defined but not used.
- **Remove unused variable**: Offered when a variable is declared in an operation but not used in the selection set.
- **Extract Fragment**: Offered when a selection of fields can be extracted into a new fragment.

## Additional Setup Tips

- **Binary Name**: Ensure `graphql-rust` is in your `$PATH`. If you built it from source, you might need to use the full path to the binary in the `cmd` field (e.g., `~/path/to/graphql-rust/target/release/graphql-rust`).
- **File Watching**: The LSP automatically registers file watchers for your schema files (as defined in your `graphql.config.yml`). You don't need additional Neovim configuration for schema reloading on save.
- **Semantic Tokens**: `graphql-rust` provides high-fidelity semantic highlighting for GraphQL blocks inside template literals. Ensure your Neovim version supports `LspTokenUpdate` (0.9+) for the best experience.
