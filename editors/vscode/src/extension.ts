import * as path from 'path';
import { workspace, ExtensionContext, commands, window, env, Uri, OutputChannel } from 'vscode';
import {
  LanguageClient,
  CloseAction,
  ErrorAction,
  LanguageClientOptions,
  ServerOptions,
  Executable
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let outputChannel: OutputChannel;

function existsSync(filePath: string): boolean {
  try {
    return require('fs').existsSync(filePath);
  } catch {
    return false;
  }
}

function findWorkspaceConfigRoot(): string | undefined {
  const folders = workspace.workspaceFolders || [];

  for (const folder of folders) {
    const rootPath = folder.uri.fsPath;
    if (
      existsSync(path.join(rootPath, 'graphox.yaml')) ||
      existsSync(path.join(rootPath, 'graphox.yml'))
    ) {
      return rootPath;
    }
  }

  return undefined;
}

function findNpmPackagePath(): string | undefined {
  const folders = workspace.workspaceFolders || [];
  
  for (const folder of folders) {
    const workspacePath = folder.uri.fsPath;
    
    // Check node_modules/.bin in workspace
    const nodeModulesBin = path.join(workspacePath, 'node_modules', '.bin');
    const graphoxBinary = path.join(nodeModulesBin, process.platform === 'win32' ? 'graphox.exe' : 'graphox');
    
    if (existsSync(graphoxBinary)) {
      return graphoxBinary;
    }
    
    // For monorepos, also check if there's a graphox-cli in node_modules
    const cliPath = path.join(nodeModulesBin, 'graphox');
    if (existsSync(cliPath)) {
      return cliPath;
    }
  }
  
  return undefined;
}

function findLocalBuildPath(): string | undefined {
  const targetDir = path.join(__dirname, '..', '..');
  const debugPath = path.join(targetDir, 'target', 'debug', 'graphox');
  const releasePath = path.join(targetDir, 'target', 'release', 'graphox');
  
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

function findServerPath(configuredPath: string): string {
  if (configuredPath) {
    return configuredPath;
  }

  // Priority 1: npm package in workspace (node_modules/.bin/graphox)
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
  return 'graphox';
}

async function showBinaryNotFoundMessage(serverPath: string, usedNpm: boolean): Promise<void> {
  const docsUri = Uri.parse('https://github.com/soundtrackyourbrand/graphox#installation');
  const configUri = Uri.parse('https://github.com/soundtrackyourbrand/graphox/blob/main/editors/vscode/README.md');

  let message: string;
  if (usedNpm) {
    message = `graphox binary from npm package was found but failed to start. Check the Output panel for details.`;
  } else {
    message = `graphox binary not found. Install via 'pnpm add @graphox/cli' or build from source.`;
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

function isBinaryNotFoundStartupError(error: unknown): boolean {
  const message = String(error);
  return message.includes('ENOENT') || message.includes('command not found');
}

async function startServer(context: ExtensionContext): Promise<void> {
  if (client) {
    outputChannel.appendLine('Stopping existing Language Client...');
    await client.stop();
    client.dispose();
    client = undefined;
  }

  const config = workspace.getConfiguration('graphox');
  const configuredPath = config.get<string>('serverPath', '').trim();
  const configRoot = findWorkspaceConfigRoot();
  if (!configRoot) {
    outputChannel.appendLine('No graphox config file found (graphox.yaml or graphox.yml). Server will not start.');
    return;
  }

  const serverPath = findServerPath(configuredPath);
  const usedNpm = serverPath.includes('node_modules');
  const logLevel = config.get<string>('logLevel', 'info');

  outputChannel.appendLine('Starting Graphox LSP server...');
  outputChannel.appendLine(`- Path: ${serverPath}`);
  outputChannel.appendLine(`- Log Level: ${logLevel}`);
  outputChannel.appendLine(`- Root: ${configRoot}`);

  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    options: {
      cwd: configRoot || workspace.workspaceFolders?.[0]?.uri.fsPath,
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
    outputChannel,
    // The LSP server handles its own file watching via dynamic registration
    errorHandler: {
      error: () => ({ action: ErrorAction.Continue }),
      closed: () => ({ action: CloseAction.DoNotRestart })
    },
    middleware: {
      executeCommand: async (command, args, next) => {
        try {
          const result = await next(command, args);
          if (command === 'graphox.runCodegen') {
            window.showInformationMessage('GraphQL Codegen completed!');
          } else if (command === 'graphox.clearCache') {
            window.showInformationMessage('GraphQL Cache cleared!');
          }
          return result;
        } catch (err) {
          if (command === 'graphox.runCodegen') {
            window.showErrorMessage(`GraphQL Codegen failed: ${err}`);
          } else if (command === 'graphox.clearCache') {
            window.showErrorMessage(`Failed to clear GraphQL Cache: ${err}`);
          }
          throw err;
        }
      }
    }
  };

  client = new LanguageClient(
    'graphox',
    'Graphox Language Server',
    serverOptions,
    clientOptions
  );

  try {
    await client.start();

    const sourceMessage = usedNpm 
      ? 'Using graphox from npm package in workspace'
      : serverPath === 'graphox'
        ? 'Using graphox from system PATH'
        : `Using local build: ${serverPath}`;
    
    window.setStatusBarMessage(`$(check) ${sourceMessage}`, 5000);
    outputChannel.appendLine('Graphox LSP server started successfully.');
  } catch (error) {
    outputChannel.appendLine(`Failed to start Graphox LSP server: ${error}`);
    if (isBinaryNotFoundStartupError(error)) {
      await showBinaryNotFoundMessage(serverPath, usedNpm);
    }
  }
}

export async function activate(context: ExtensionContext): Promise<void> {
  outputChannel = window.createOutputChannel('Graphox');
  context.subscriptions.push(outputChannel);

  // Register commands before starting the server so they are always available
  // Only register client-side commands here. Server-side commands are registered
  // automatically by the LanguageClient via executeCommandProvider.

  context.subscriptions.push(
    commands.registerCommand('graphox.restartServer', async () => {
      await startServer(context);
      window.showInformationMessage(`Graphox server restarted.`);
    })
  );

  context.subscriptions.push(
    workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration('graphox.serverPath') || e.affectsConfiguration('graphox.logLevel')) {
        const action = await window.showInformationMessage(
          'Graphox configuration changed. Would you like to restart the server?',
          'Restart Now'
        );
        if (action === 'Restart Now') {
          await startServer(context);
        }
      }
    })
  );

  await startServer(context);
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
