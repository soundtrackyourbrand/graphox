const path = require('path');
const fs = require('fs');

function normalize(s) {
  return s.replace(/\s+/g, '');
}

/**
 * Convert emitExtensions config to file extension string
 * @param {string|undefined} emitExtensions - One of: "none", "ts", "dts", "js"
 * @returns {string} The file extension to append (e.g., ".ts", ".js", or "")
 */
function getExtension(emitExtensions) {
  switch (emitExtensions) {
    case 'ts':
      return '.ts';
    case 'dts':
      return '.d.ts';
    case 'js':
      return '.js';
    case 'none':
    case undefined:
    case null:
    default:
      return '';
  }
}

module.exports = function (babel) {
  const { types: t } = babel;

  return {
    name: '@graphox/babel-plugin',
    visitor: {
      Program: {
        enter(programPath, state) {
          const {
            manifestPath,
            manifestData,
            outputDir,
            graphqlImportPaths = [],
            emitExtensions,
          } = state.opts;

          if (!outputDir) {
            throw new Error('outputDir is required for @graphox/babel-plugin');
          }

          const extension = getExtension(emitExtensions);

          let entries = [];
          if (manifestData) {
            entries = manifestData;
          } else if (manifestPath) {
            try {
              const content = fs.readFileSync(manifestPath, 'utf8');
              entries = JSON.parse(content);
            } catch (e) {
              // Ignore missing manifest during build if necessary
            }
          }

          const manifest = new Map();
          const documentNameToEntry = new Map();
          for (const entry of entries) {
            manifest.set(normalize(entry.source), entry);
            if (entry.name) {
              documentNameToEntry.set(entry.name, entry);
            }
          }

          const currentFile = state.file.opts.filename;
          const absoluteOutputDir = path.resolve(outputDir);
          const absoluteIndexPath = path.join(absoluteOutputDir, 'index');
          const absoluteEntrypointPath = path.join(absoluteOutputDir, 'graphql');

          // If processing the entrypoint itself, clear it
          if (currentFile) {
            const currentFileNoExt = currentFile.replace(/\.(js|ts)x?$/, '');
            if (currentFileNoExt === absoluteEntrypointPath) {
              programPath.node.body = [
                t.exportNamedDeclaration(
                  t.variableDeclaration('const', [
                    t.variableDeclarator(
                      t.identifier('graphql'),
                      t.arrowFunctionExpression([], t.nullLiteral())
                    ),
                  ])
                ),
                t.exportNamedDeclaration(
                  t.variableDeclaration('const', [
                    t.variableDeclarator(t.identifier('gql'), t.identifier('graphql')),
                  ])
                ),
              ];
              return;
            }
          }

          const isOurGraphqlPath = (src) => {
            if (graphqlImportPaths.includes(src)) return true;
            const srcNoExt = src.replace(/\.(js|ts)x?$/, '');
            if (graphqlImportPaths.includes(srcNoExt)) return true;

            if (src.startsWith('#') && src.includes('graphql')) return true;

            if (currentFile && (src.startsWith('.') || src.startsWith('/'))) {
              const absoluteSrc = path.resolve(path.dirname(currentFile), srcNoExt);
              if (absoluteSrc === absoluteEntrypointPath || absoluteSrc === absoluteIndexPath) {

                return true;
              }
              // Handle directory import resolving to index
              const absoluteSrcIndex = path.join(absoluteSrc, 'index');
              if (absoluteSrcIndex === absoluteIndexPath) {
                return true;
              }
            }

            return false;
          };

          const graphqlIds = new Set();
          // newImports stores: localName -> { sourcePath, importedName }
          // importedName is needed for aliased imports like { GetUserDocument as MyDoc }
          const newImports = new Map();

          // First pass: identify imports from our graphql.ts or index.ts
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              if (isOurGraphqlPath(src)) {
                const importIsTypeOnly = importPath.node.importKind === 'type';
                importPath.get('specifiers').forEach((specifier) => {
                  if (specifier.isImportSpecifier()) {
                    // Handle both Identifier and StringLiteral for imported name
                    const importedName = t.isIdentifier(specifier.node.imported) 
                      ? specifier.node.imported.name 
                      : specifier.node.imported.value;
                    const localName = specifier.node.local.name;
                    const specifierIsTypeOnly = specifier.node.importKind === 'type' || importIsTypeOnly;
                    
                    if (importedName === 'graphql' || importedName === 'gql') {
                      graphqlIds.add(specifier.scope.getBinding(localName));
                    } else if (documentNameToEntry.has(importedName) && !specifierIsTypeOnly) {
                      // Only rewrite non-type-only imports of document names
                      const entry = documentNameToEntry.get(importedName);
                      const codegenAbsPath = path.join(absoluteOutputDir, entry.path);
                      let relPath = path.relative(path.dirname(currentFile), codegenAbsPath);
                      if (!relPath.startsWith('.') && !relPath.startsWith('/')) {
                        relPath = './' + relPath;
                      }
                      // Append the emit extension
                      relPath += extension;
                      newImports.set(localName, { sourcePath: relPath, importedName });
                    }
                  }
                });
              }
            },
          });

          // Second pass: transform graphql() calls
          programPath.traverse({
            CallExpression(callPath) {
              const callee = callPath.get('callee');
              if (callee.isIdentifier()) {
                const binding = callPath.scope.getBinding(callee.node.name);
                if (graphqlIds.has(binding)) {
                  const arg = callPath.node.arguments[0];
                  let source = null;

                  if (t.isTemplateLiteral(arg)) {
                    if (arg.quasis.length === 1) {
                      source = arg.quasis[0].value.cooked || arg.quasis[0].value.raw;
                    }
                  } else if (t.isStringLiteral(arg)) {
                    source = arg.value;
                  }

                  if (source) {
                    const normalizedSource = normalize(source);
                    const entry = manifest.get(normalizedSource);
                    if (entry) {
                      const codegenAbsPath = path.join(absoluteOutputDir, entry.path);
                      let relPath = path.relative(path.dirname(currentFile), codegenAbsPath);
                      if (!relPath.startsWith('.') && !relPath.startsWith('/')) {
                        relPath = './' + relPath;
                      }
                      // Append the emit extension
                      relPath += extension;

                      newImports.set(entry.name, { sourcePath: relPath, importedName: entry.name });
                      callPath.replaceWith(t.identifier(entry.name));
                    }
                  }
                }
              }
            },
          });

          // Third pass: remove ALL imports from graphql.ts/index.ts
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              if (isOurGraphqlPath(src)) {
                importPath.remove();
              }
            },
          });

          // Add new imports at the top (sorted alphabetically by local name)
          const sortedNewImports = Array.from(newImports.entries()).sort((a, b) => a[0].localeCompare(b[0]));
          for (const [localName, { sourcePath, importedName }] of sortedNewImports) {
            const specifier = t.importSpecifier(
              t.identifier(localName),
              t.identifier(importedName)
            );
            programPath.node.body.unshift(
              t.importDeclaration([specifier], t.stringLiteral(sourcePath))
            );
          }
        },
      },
    },
  };
};
