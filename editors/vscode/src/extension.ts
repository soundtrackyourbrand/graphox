import * as path from 'path';
import { execFile } from 'child_process';
import { promisify } from 'util';
import {
  workspace,
  ExtensionContext,
  commands,
  window,
  env,
  Uri,
  OutputChannel,
  RelativePattern,
  Disposable
} from 'vscode';
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
let versionMonitorDisposables: Disposable[] = [];
let versionCheckTimer: NodeJS.Timeout | undefined;
let runningServerVersion: string | undefined;
let monitoredConfigRoot: string | undefined;
let monitoredConfiguredPath = '';
let lastVersionWarningKey: string | undefined;

const execFileAsync = promisify(execFile);
const GRAPHOX_PACKAGE_NAME = '@graphox/cli';

interface PackageManifest {
  version?: string;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
}

interface InstalledCliInfo {
  binaryPath: string;
  packageJsonPath: string;
  version: string;
}

interface ParsedVersion {
  major: number;
  minor: number;
  patch: number;
}

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

function readJsonFile<T>(filePath: string): T | undefined {
  try {
    const content = require('fs').readFileSync(filePath, 'utf8');
    return JSON.parse(content) as T;
  } catch {
    return undefined;
  }
}

function candidateWorkspaceRoots(preferredRoot?: string): string[] {
  const roots = (workspace.workspaceFolders || []).map((folder) => folder.uri.fsPath);
  if (!preferredRoot) {
    return roots;
  }

  const ordered = [preferredRoot, ...roots.filter((root) => root !== preferredRoot)];
  return Array.from(new Set(ordered));
}

function findNpmPackagePath(preferredRoot?: string): string | undefined {
  for (const workspacePath of candidateWorkspaceRoots(preferredRoot)) {
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

function findServerPath(configuredPath: string, configRoot?: string): string {
  if (configuredPath) {
    return configuredPath;
  }

  // Priority 1: npm package in workspace (node_modules/.bin/graphox)
  const npmPath = findNpmPackagePath(configRoot);
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

function parseVersion(value: string): ParsedVersion | undefined {
  const match = value.match(/(\d+)\.(\d+)\.(\d+)/);
  if (!match) {
    return undefined;
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3])
  };
}

function compareVersions(left: string, right: string): number | undefined {
  const parsedLeft = parseVersion(left);
  const parsedRight = parseVersion(right);
  if (!parsedLeft || !parsedRight) {
    return undefined;
  }

  if (parsedLeft.major !== parsedRight.major) {
    return parsedLeft.major - parsedRight.major;
  }
  if (parsedLeft.minor !== parsedRight.minor) {
    return parsedLeft.minor - parsedRight.minor;
  }
  return parsedLeft.patch - parsedRight.patch;
}

function extractVersion(text: string): string | undefined {
  const match = text.match(/(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)/);
  return match?.[1];
}

function getDeclaredCliSpec(configRoot: string): string | undefined {
  const manifest = readJsonFile<PackageManifest>(path.join(configRoot, 'package.json'));
  if (!manifest) {
    return undefined;
  }

  const sections = [
    manifest.dependencies,
    manifest.devDependencies,
    manifest.optionalDependencies,
    manifest.peerDependencies
  ];

  for (const section of sections) {
    const spec = section?.[GRAPHOX_PACKAGE_NAME];
    if (spec) {
      return spec;
    }
  }

  return undefined;
}

function extractMinimumVersion(spec: string): string | undefined {
  let normalized = spec.trim();
  if (!normalized || normalized === '*' || normalized === 'latest' || normalized === 'workspace:*') {
    return undefined;
  }

  if (normalized.startsWith('workspace:')) {
    normalized = normalized.slice('workspace:'.length).trim();
  }

  if (!normalized || /^(file|link|git|github|https?):/.test(normalized)) {
    return undefined;
  }

  return extractVersion(normalized);
}

function getInstalledCliInfo(configRoot: string): InstalledCliInfo | undefined {
  const packageJsonPath = path.join(configRoot, 'node_modules', '@graphox', 'cli', 'package.json');
  const binaryPath = path.join(
    configRoot,
    'node_modules',
    '.bin',
    process.platform === 'win32' ? 'graphox.exe' : 'graphox'
  );

  if (!existsSync(packageJsonPath) || !existsSync(binaryPath)) {
    return undefined;
  }

  const manifest = readJsonFile<PackageManifest>(packageJsonPath);
  if (!manifest?.version) {
    return undefined;
  }

  return {
    binaryPath,
    packageJsonPath,
    version: manifest.version
  };
}

async function getBinaryVersion(command: string, cwd?: string): Promise<string | undefined> {
  try {
    const { stdout, stderr } = await execFileAsync(command, ['--version'], {
      cwd,
      encoding: 'utf8'
    });
    return extractVersion(`${stdout}\n${stderr}`);
  } catch (error) {
    outputChannel.appendLine(`Failed to read Graphox version from '${command} --version': ${error}`);
    return undefined;
  }
}

function clearVersionMonitoring(): void {
  if (versionCheckTimer) {
    clearTimeout(versionCheckTimer);
    versionCheckTimer = undefined;
  }

  for (const disposable of versionMonitorDisposables) {
    disposable.dispose();
  }
  versionMonitorDisposables = [];
  runningServerVersion = undefined;
  monitoredConfigRoot = undefined;
  monitoredConfiguredPath = '';
  lastVersionWarningKey = undefined;
}

async function showVersionWarning(message: string, key: string, action?: 'Restart Server'): Promise<void> {
  if (lastVersionWarningKey === key) {
    return;
  }
  lastVersionWarningKey = key;

  const selectedAction = action
    ? await window.showWarningMessage(message, action)
    : await window.showWarningMessage(message);

  if (selectedAction === 'Restart Server') {
    await commands.executeCommand('graphox.restartServer');
  }
}

async function checkVersionDrift(): Promise<void> {
  if (!monitoredConfigRoot || monitoredConfiguredPath) {
    return;
  }

  const configRoot = monitoredConfigRoot;
  const installed = getInstalledCliInfo(configRoot);
  const declaredSpec = getDeclaredCliSpec(configRoot);
  const declaredMinimum = declaredSpec ? extractMinimumVersion(declaredSpec) : undefined;

  if (declaredSpec) {
    if (!installed) {
      await showVersionWarning(
        `Workspace package.json declares ${GRAPHOX_PACKAGE_NAME} (${declaredSpec}) but it is not installed in node_modules. Run pnpm install, then restart Graphox.`,
        `install-missing:${configRoot}:${declaredSpec}`
      );
      return;
    }

    if (declaredMinimum) {
      const installedVsDeclared = compareVersions(installed.version, declaredMinimum);
      if (installedVsDeclared !== undefined && installedVsDeclared < 0) {
        await showVersionWarning(
          `Workspace package.json expects ${GRAPHOX_PACKAGE_NAME} ${declaredSpec}, but node_modules has ${installed.version}. Run pnpm install, then restart Graphox.`,
          `install-outdated:${configRoot}:${declaredSpec}:${installed.version}`
        );
        return;
      }
    }
  }

  if (installed && runningServerVersion) {
    const versionComparison = compareVersions(runningServerVersion, installed.version);
    const versionsDiffer = versionComparison !== undefined
      ? versionComparison !== 0
      : runningServerVersion !== installed.version;

    if (versionsDiffer) {
      await showVersionWarning(
        `Graphox is still running ${runningServerVersion}, but the workspace now has ${installed.version} installed in node_modules. Restart the server to use the updated binary.`,
        `restart:${configRoot}:${runningServerVersion}:${installed.version}`,
        'Restart Server'
      );
      return;
    }
  }

  lastVersionWarningKey = undefined;
}

function scheduleVersionDriftCheck(): void {
  if (versionCheckTimer) {
    clearTimeout(versionCheckTimer);
  }

  versionCheckTimer = setTimeout(() => {
    void checkVersionDrift();
  }, 250);
}

function setupVersionMonitoring(configRoot: string, configuredPath: string): void {
  monitoredConfigRoot = configRoot;
  monitoredConfiguredPath = configuredPath;

  if (configuredPath) {
    return;
  }

  const manifestWatcher = workspace.createFileSystemWatcher(
    new RelativePattern(configRoot, 'package.json')
  );
  const installedCliWatcher = workspace.createFileSystemWatcher(
    new RelativePattern(configRoot, 'node_modules/@graphox/cli/package.json')
  );

  versionMonitorDisposables.push(
    manifestWatcher,
    installedCliWatcher,
    manifestWatcher.onDidChange(() => scheduleVersionDriftCheck()),
    manifestWatcher.onDidCreate(() => scheduleVersionDriftCheck()),
    manifestWatcher.onDidDelete(() => scheduleVersionDriftCheck()),
    installedCliWatcher.onDidChange(() => scheduleVersionDriftCheck()),
    installedCliWatcher.onDidCreate(() => scheduleVersionDriftCheck()),
    installedCliWatcher.onDidDelete(() => scheduleVersionDriftCheck()),
    window.onDidChangeWindowState((state) => {
      if (state.focused) {
        scheduleVersionDriftCheck();
      }
    })
  );

  scheduleVersionDriftCheck();
}

async function startServer(context: ExtensionContext): Promise<void> {
  clearVersionMonitoring();

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

  const serverPath = findServerPath(configuredPath, configRoot);
  const usedNpm = serverPath.includes('node_modules');
  const logLevel = config.get<string>('logLevel', 'info');
  const rustBacktrace = config.get<string>('rustBacktrace', '').trim();

  outputChannel.appendLine('Starting Graphox LSP server...');
  outputChannel.appendLine(`- Path: ${serverPath}`);
  outputChannel.appendLine(`- Log Level: ${logLevel}`);
  outputChannel.appendLine(`- RUST_BACKTRACE: ${rustBacktrace || '(unset)'}`);
  outputChannel.appendLine(`- Root: ${configRoot}`);

  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    options: {
      cwd: configRoot || workspace.workspaceFolders?.[0]?.uri.fsPath,
      env: {
        ...process.env,
        RUST_LOG: logLevel === 'debug' || logLevel === 'trace' ? logLevel : undefined,
        RUST_BACKTRACE: rustBacktrace || undefined
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
    runningServerVersion = await getBinaryVersion(
      serverPath,
      configRoot || workspace.workspaceFolders?.[0]?.uri.fsPath
    );
    if (runningServerVersion) {
      outputChannel.appendLine(`- Server Version: ${runningServerVersion}`);
    }
    setupVersionMonitoring(configRoot, configuredPath);

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
      if (e.affectsConfiguration('graphox.serverPath') || e.affectsConfiguration('graphox.logLevel') || e.affectsConfiguration('graphox.rustBacktrace')) {
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
  clearVersionMonitoring();
  if (!client) {
    return undefined;
  }
  return client.stop();
}
