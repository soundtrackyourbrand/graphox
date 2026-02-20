# IDE Setup: IntelliJ / JetBrains

We use the [LSP4IJ](https://github.com/redhat-developer/lsp4ij) plugin to connect our custom GraphQL LSP to IntelliJ (and other JetBrains IDEs like WebStorm, PyCharm, etc.). This requires no custom plugin code, just a quick manual configuration.

## Prerequisites
* **IntelliJ Platform IDE**: Version 2023.2 or newer is recommended.

## Installation Methods

### Method 1: Using npm/pnpm Package (Recommended for Projects)

This method uses the `@graphox/cli` package installed in your project. It's the easiest setup and ensures version consistency.

**Step 1: Install the LSP Client Plugin**
1.  Open **Settings** (Windows/Linux: `Ctrl+Alt+S`, macOS: `Cmd+,`).
2.  Navigate to **Plugins**.
3.  Search for **LSP4IJ** (published by Red Hat).
4.  Click **Install** and restart the IDE if prompted.

**Step 2: Configure the Language Server**
1.  Open **Settings** again.
2.  Navigate to **Languages & Frameworks > Language Server Protocol > Server Definitions**.
3.  Click the **+** (plus) icon in the toolbar to add a new server definition.
4.  Fill in the dropdown settings as follows:

    * **Name:** `GraphQL-Rust-LSP` (or any name you prefer)
    * **Type:** `Executable` (default)

**Step 3: Set the Run Configuration**

#### For pnpm (Recommended)
In the **Server configuration** tab (right pane):

* **Command:**
    * `pnpm`
* **Args:**
    ```
    exec
    graphox
    lsp
    ```

#### For npm
* **Command:**
    * `npm`
* **Args:**
    ```
    exec
    --
    graphox
    lsp
    ```

#### For Yarn
* **Command:**
    * `yarn`
* **Args:**
    ```
    exec
    graphox
    lsp
    ```

**Step 4: Set File Types**
1.  In the same configuration, find **File Types**.
2.  Add the following patterns (one by one or comma-separated):
    
    `graphql`, `ts`, `tsx`, `mts`, `cts`, `js`, `jsx`, `mjs`, `cjs`

**Step 5: Verify & Apply**
1.  Click **Apply** and **OK**.
2.  Open a `.graphql` or `.ts` file in your project.
3.  Look at the bottom status bar. You should see a small LSP icon ( `{}` or similar). If you click it, it should say "GraphQL-Rust-LSP: Running".

---

### Method 2: Using Binary from PATH

If you have `Graphox` installed globally (via Homebrew, direct download, etc.):

**Command:** `Graphox`
**Args:** `lsp`

---

### Method 3: Using Local Build

For developing `Graphox` itself, point directly to your local build:

**Command:**
* macOS/Linux: `/path/to/graphox/target/release/graphox`
* Windows: `C:\path\to\graphox\target\release\graphox.exe`

**Args:** `lsp`

---

## Troubleshooting

- **Server fails to start**: Open the "LSP Consoles" tool window (View > Tool Windows > LSP Consoles) to see error output.
- **Version issues**: Ensure the npm package version matches your expectations. Check with `pnpm list @graphox/cli`.
- **Binary not found**: Use Method 3 with an absolute path to the binary.
- **Monorepo issues**: Make sure you're in a workspace folder where `graphox-cli` is installed.

## Notes

- **Automatic updates**: Using the npm package method means updates happen when you update dependencies.
- **Multiple versions**: In monorepos, each project can use a different version by installing locally.
- **Performance**: Local builds (Method 3) may be faster for development if you're modifying the Rust code.
