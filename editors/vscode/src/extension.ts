import * as path from 'path';
import { workspace, ExtensionContext, commands, window, env, Uri } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

function existsSync(filePath: string): boolean {
  try {
    require('fs').existsSync(filePath);
    return true;
  } catch {
    return false;
  }
}

function findNpmPackagePath(): string | undefined {
  const folders = workspace.workspaceFolders || [];
  
  for (const folder of folders) {
    const workspacePath = folder.uri.fsPath;
    
    // Check node_modules/.bin in workspace
    const nodeModulesBin = path.join(workspacePath, 'node_modules', '.bin');
    const graphqlRustBinary = path.join(nodeModulesBin, process.platform === 'win32' ? 'graphql-rust.exe' : 'graphql-rust');
    
    if (existsSync(graphqlRustBinary)) {
      return graphqlRustBinary;
    }
    
    // For monorepos, also check if there's a graphql-rust-cli in node_modules
    const cliPath = path.join(nodeModulesBin, 'graphql-rust');
    if (existsSync(cliPath)) {
      return cliPath;
    }
  }
  
  return undefined;
}

function findLocalBuildPath(): string | undefined {
  const targetDir = path.join(__dirname, '..', '..');
  const debugPath = path.join(targetDir, 'target', 'debug', 'graphql-rust');
  const releasePath = path.join(targetDir, 'target', 'release', 'graphql-rust');
  
  if (process.platform === 'win32') {
    const debugPathExe = debugPath + '.exe';
    const releasePathExe = releasePath + '.exe';
    
    if (existsSync(debugPathExe)) {
      return debugPathExe;
    }
    if (existsSync(releasePathExe)) {
      return releasePathExe;
    }
  } else {
    if (existsSync(debugPath)) {
      return debugPath;
    }
    if (existsSync(releasePath)) {
      return releasePath;
    }
  }
  
  return undefined;
}

function findServerPath(context: ExtensionContext): string {
  const config = workspace.getConfiguration('graphql-rust');
  const configuredPath = config.get<string>('serverPath', '').trim();

  if (configuredPath) {
    return configuredPath;
  }

  // Priority 1: npm package in workspace (node_modules/.bin/graphql-rust)
  const npmPath = findNpmPackagePath();
  if (npmPath) {
    return npmPath;
  }

  // Priority 2: Local build (target/release or target/debug)
  const localBuildPath = findLocalBuildPath();
  if (localBuildPath) {
    return localBuildPath;
  }

  // Priority 3: System PATH
  return 'graphql-rust';
}

async function showBinaryNotFoundMessage(serverPath: string, usedNpm: boolean): Promise<void> {
  const docsUri = require('vscode').Uri.parse('https://github.com/YOUR_USERNAME/graphql-rust#installation');
  const configUri = require('vscode').Uri.parse('https://github.com/YOUR_USERNAME/graphql-rust/blob/main/editors/vscode/README.md');

  let message: string;
  if (usedNpm) {
    message = `graphql-rust binary from npm package was found but failed to start. Check the Output panel for details.`;
  } else {
    message = `graphql-rust binary not found. Install via 'pnpm add graphql-rust-cli' or build from source.`;
  }

  const action = await window.showErrorMessage(
    message,
    'Set Binary Path',
    'View Documentation'
  );

  if (action === 'Set Binary Path') {
    await env.openExternal(docsUri);
  } else if (action === 'View Documentation') {
    await env.openExternal(configUri);
  }
}

export async function activate(context: ExtensionContext): Promise<void> {
  const serverPath = findServerPath(context);
  const usedNpm = serverPath.includes('node_modules');

  const logLevel = workspace.getConfiguration('graphql-rust').get<string>('logLevel', 'info');

  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    options: {
      env: {
        ...process.env,
        RUST_LOG: logLevel === 'debug' || logLevel === 'trace' ? logLevel : undefined
      }
    }
  };

  const serverOptions: ServerOptions = {
    run,
    debug: run
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'graphql' },
      { scheme: 'file', language: 'typescript' },
      { scheme: 'file', language: 'typescriptreact' },
      { scheme: 'file', language: 'javascript' },
      { scheme: 'file', language: 'javascriptreact' }
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/*.{graphql,ts,tsx,js,jsx}')
    }
  };

  client = new LanguageClient(
    'graphqlRust',
    'GraphQL Rust Language Server',
    serverOptions,
    clientOptions
  );

  try {
    await client.start();

    const sourceMessage = usedNpm 
      ? 'Using graphql-rust from npm package in workspace'
      : serverPath === 'graphql-rust'
        ? 'Using graphql-rust from system PATH'
        : `Using local build: ${serverPath}`;
    
    window.setStatusBarMessage(`$(check) ${sourceMessage}`, 5000);

    context.subscriptions.push(
      commands.registerCommand('graphql.runCodegen', async () => {
        try {
          await client?.sendRequest('workspace/executeCommand', {
            command: 'graphql.runCodegen',
            arguments: []
          });
          window.showInformationMessage('GraphQL Codegen completed!');
        } catch (err) {
          window.showErrorMessage(`GraphQL Codegen failed: ${err}`);
        }
      })
    );

    context.subscriptions.push(
      commands.registerCommand('graphql.clearCache', async () => {
        try {
          await client?.sendRequest('workspace/executeCommand', {
            command: 'graphql.clearCache',
            arguments: []
          });
          window.showInformationMessage('GraphQL Cache cleared!');
        } catch (err) {
          window.showErrorMessage(`Failed to clear GraphQL Cache: ${err}`);
        }
      })
    );

    context.subscriptions.push(
      commands.registerCommand('graphql.restartServer', async () => {
        await client?.stop();
        client?.dispose();
        client = new LanguageClient(
          'graphqlRust',
          'GraphQL Rust Language Server',
          serverOptions,
          clientOptions
        );
        await client.start();
        const newPath = findServerPath(context);
        const newUsedNpm = newPath.includes('node_modules');
        const newSourceMessage = newUsedNpm
          ? 'Using graphql-rust from npm package in workspace'
          : newPath === 'graphql-rust'
            ? 'Using graphql-rust from system PATH'
            : `Using local build: ${newPath}`;
        window.showInformationMessage(`GraphQL Rust server restarted. ${newSourceMessage}`);
      })
    );

  } catch (error) {
    await showBinaryNotFoundMessage(serverPath, usedNpm);
    throw error;
  }
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
