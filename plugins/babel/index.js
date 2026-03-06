const path = require('path');
const fs = require('fs');

function normalize(s) {
  return s.replace(/\s+/g, '');
}

function toPosixPath(str) {
  return str.replace(/\\/g, '/');
}

function stripScriptExtension(filePath) {
  return filePath.replace(/(\.d)?\.(mjs|cjs|js|jsx|ts|tsx)$/, '');
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
          const currentDir = currentFile ? path.dirname(currentFile) : null;

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
            src = toPosixPath(src);
            if (graphqlImportPaths.includes(src)) return true;
            const srcNoExt = stripScriptExtension(src);
            if (graphqlImportPaths.includes(srcNoExt)) return true;

            if (currentDir) {
              let absoluteSrc = null;
              try {
                if (src.startsWith('.') || path.isAbsolute(src)) {
                  absoluteSrc = path.resolve(currentDir, srcNoExt);
                } else {
                  absoluteSrc = stripScriptExtension(
                    require.resolve(src, { paths: [currentDir] }),
                  );
                }
              } catch (_) {}

              if (absoluteSrc) {
                if (
                  absoluteSrc === absoluteEntrypointPath ||
                  absoluteSrc === absoluteIndexPath
                ) {
                  return true;
                }
                // Handle directory import resolving to index
                const absoluteSrcIndex = path.join(absoluteSrc, 'index');
                if (absoluteSrcIndex === absoluteIndexPath) {
                  return true;
                }
              }
            }

            return false;
          };

          const graphqlIds = new Set();
          // newImports stores: localName -> { sourcePath, importedName }
          const newImports = new Map();
          // Map from original document name to unique local name in this file
          const documentNameToLocalName = new Map();

          const getLocalName = (documentName, scope) => {
            if (documentNameToLocalName.has(documentName)) {
              return documentNameToLocalName.get(documentName);
            }

            let uniqueName = documentName;
            // If the name is already in scope, we MUST generate a unique one.
            // We check if it's a binding that is NOT one of our graphql imports.
            const binding = scope.getBinding(documentName);
            let isColliding = false;

            if (binding) {
              // It's a collision if it's NOT an import from our graphql path
              const isOurImport =
                (t.isImportSpecifier(binding.path.node) || t.isImportDefaultSpecifier(binding.path.node) || t.isImportNamespaceSpecifier(binding.path.node)) &&
                isOurGraphqlPath(binding.path.parent.source.value);

              if (!isOurImport) {
                isColliding = true;
              }
            } else if (scope.hasReference(documentName)) {
              // If there's a reference but no binding, it might be a global or
              // something Babel doesn't fully track as a binding yet.
              isColliding = true;
            }

            if (isColliding) {
              uniqueName = scope.generateUid(documentName);
            }

            documentNameToLocalName.set(documentName, uniqueName);
            return uniqueName;
          };

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
                      relPath = toPosixPath(relPath);
                      if (!relPath.startsWith('.') && !path.isAbsolute(relPath)) {
                        relPath = './' + relPath;
                      }
                      // Append the emit extension
                      relPath += extension;

                      // If the original import was aliased (e.g. import { D as MyD }),
                      // we want to keep that alias if possible.
                      let targetLocalName = localName;
                      if (localName === importedName) {
                        // It was NOT aliased, check for collisions
                        targetLocalName = getLocalName(importedName, programPath.scope);
                      } else {
                        // It WAS aliased. We should keep the alias but ensure it's recorded
                        // so that subsequent graphql(`...`) calls use the same alias.
                        documentNameToLocalName.set(importedName, localName);
                      }

                      newImports.set(targetLocalName, { sourcePath: relPath, importedName });

                      if (localName !== targetLocalName) {
                        specifier.scope.rename(localName, targetLocalName);
                      }
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
                      relPath = toPosixPath(relPath);
                      if (!relPath.startsWith('.') && !path.isAbsolute(relPath)) {
                        relPath = './' + relPath;
                      }
                      // Append the emit extension
                      relPath += extension;

                      const uniqueLocalName = getLocalName(entry.name, callPath.scope);
                      newImports.set(uniqueLocalName, { sourcePath: relPath, importedName: entry.name });
                      callPath.replaceWith(t.identifier(uniqueLocalName));
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