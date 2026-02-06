# IDE Setup: IntelliJ / JetBrains

We use the [LSP4IJ](https://github.com/redhat-developer/lsp4ij) plugin to connect our custom GraphQL LSP to IntelliJ (and other JetBrains IDEs like WebStorm, PyCharm, etc.). This requires no custom plugin code, just a quick manual configuration.

## Prerequisites
* **IntelliJ Platform IDE**: Version 2023.2 or newer is recommended.

## Step-by-Step Configuration

### 1. Install the LSP Client Plugin
1.  Open **Settings** (Windows/Linux: `Ctrl+Alt+S`, macOS: `Cmd+,`).
2.  Navigate to **Plugins**.
3.  Search for **LSP4IJ** (published by Red Hat).
4.  Click **Install** and restart the IDE if prompted.

### 2. Configure the Language Server
1.  Open **Settings** again.
2.  Navigate to **Languages & Frameworks > Language Server Protocol > Server Definitions**.
3.  Click the **+** (plus) icon in the toolbar to add a new server definition.
4.  Fill in the dropdown settings as follows:

    * **Name:** `GraphQL-Rust-LSP` (or any name you prefer)
    * **Type:** `Executable` (default)

### 3. Set the Run Configuration
In the **Server configuration** tab (right pane), enter the following details:

* **Command:**
    * *If running from the repo root:*
        `pnpm`
    * *Args:*
        `exec`
        `graphql-rust`
        `lsp`

* **File Types:**
    Add the file extensions you want this LSP to manage. You will need to add them one by one or comma-separated depending on the UI version, but ensure all these are covered:
    
    `graphql`, `ts`, `tsx`, `mts`, `cts`, `js`, `jsx`, `mjs`, `cjs`

### 4. Verify & Apply
1.  Click **Apply** and **OK**.
2.  Open a `.graphql` or `.ts` file in your project.
3.  Look at the bottom status bar. You should see a small LSP icon ( `{}` or similar). If you click it, it should say "GraphQL-Rust-LSP: Running".
4.  **Troubleshooting:** If the server fails to start, open the "LSP Consoles" tool window (View > Tool Windows > LSP Consoles) to see the error output.
