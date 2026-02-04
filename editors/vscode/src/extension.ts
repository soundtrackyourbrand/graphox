import * as path from 'path';
import { workspace, ExtensionContext, commands, window } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  // Try to find the binary in the target folder relative to the extension
  // In a real production extension, you might bundle the binary or ask the user for a path
  const serverPath = context.asAbsolutePath(path.join('..', '..', 'target', 'debug', 'graphql-rust'));

  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    options: {
      env: {
        ...process.env,
        RUST_LOG: 'debug'
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
      { scheme: 'file', language: 'typescriptreact' }
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/*.{graphql,ts,tsx}')
    }
  };

  client = new LanguageClient(
    'graphqlRust',
    'GraphQL Rust Language Server',
    serverOptions,
    clientOptions
  );

  // Register custom commands
  context.subscriptions.push(
    commands.registerCommand('graphql.runCodegen', () => {
      client.sendRequest('workspace/executeCommand', {
        command: 'graphql.runCodegen',
        arguments: []
      }).then(
        () => window.showInformationMessage('GraphQL Codegen completed!'),
        (err) => window.showErrorMessage(`GraphQL Codegen failed: ${err}`)
      );
    })
  );

  context.subscriptions.push(
    commands.registerCommand('graphql.clearCache', () => {
      client.sendRequest('workspace/executeCommand', {
        command: 'graphql.clearCache',
        arguments: []
      }).then(
        () => window.showInformationMessage('GraphQL Cache cleared!'),
        (err) => window.showErrorMessage(`Failed to clear GraphQL Cache: ${err}`)
      );
    })
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
