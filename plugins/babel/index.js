const path = require('path');
const fs = require('fs');
const {
  findNearestFile,
  resolveTsConfigPaths,
  resolvePackageJsonImports
} = require('./utils');

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
          let {
            manifestPath,
            manifestData,
            outputDir,
            graphqlImportPaths: configuredImportPaths = [],
            emitExtensions,
          } = state.opts;

          if (!outputDir) {
            throw new Error('outputDir is required for @graphox/babel-plugin');
          }

          const currentFile = state.file.opts.filename;
          const currentDir = currentFile ? path.dirname(currentFile) : process.cwd();

          const tsconfigPath = findNearestFile(currentDir, 'tsconfig.json');
          const pkgJsonPath = findNearestFile(currentDir, 'package.json');
          const rootDir = (pkgJsonPath || tsconfigPath) ? path.dirname(pkgJsonPath || tsconfigPath) : process.cwd();

          const absoluteOutputDir = path.isAbsolute(outputDir)
            ? outputDir
            : path.resolve(rootDir, outputDir);

          // Resolve manifestPath relative to rootDir if provided as relative
          if (manifestPath && !path.isAbsolute(manifestPath)) {
            manifestPath = path.resolve(rootDir, manifestPath);
          }

          // Default manifestPath if not provided
          if (!manifestPath && !manifestData) {
            manifestPath = path.join(absoluteOutputDir, 'manifest.json');
          }

          // Auto-detect import paths from tsconfig.json and package.json
          const importPathsSet = new Set(configuredImportPaths.map(toPosixPath));

          if (tsconfigPath) {
            const paths = resolveTsConfigPaths(tsconfigPath, absoluteOutputDir);
            for (const p of paths) {
              importPathsSet.add(toPosixPath(p));
            }
          }

          if (pkgJsonPath) {
             const imports = resolvePackageJsonImports(pkgJsonPath, absoluteOutputDir);
             for (const p of imports) {
               importPathsSet.add(toPosixPath(p));
             }
          }

          const graphqlImportPaths = Array.from(importPathsSet);
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
            const normalizedSource = normalize(entry.source);
            if (!manifest.has(normalizedSource)) {
              manifest.set(normalizedSource, entry);
            }
            if (entry.name) {
              documentNameToEntry.set(entry.name, entry);
            }
          }

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
            src = toPosixPath(src);
            if (graphqlImportPaths.includes(src)) return true;
            const srcNoExt = stripScriptExtension(src);
            if (graphqlImportPaths.includes(srcNoExt)) return true;

            // Support prefix matching for directory aliases discovered from tsconfig/package.json
            for (const p of graphqlImportPaths) {
              if (p.endsWith('/') && src.startsWith(p)) {
                const subPath = src.slice(p.length);
                const subPathNoExt = stripScriptExtension(subPath);
                if (subPathNoExt === 'graphql' || subPathNoExt === 'index') {
                  return true;
                }
              }
            }

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
                  if (specifier.isImportDefaultSpecifier() || specifier.isImportNamespaceSpecifier()) {
                    if (!importIsTypeOnly) {
                      const importKind = specifier.isImportDefaultSpecifier() ? 'default' : 'namespace';
                      throw specifier.buildCodeFrameError(
                        `@graphox/babel-plugin could not fully rewrite this ${importKind} import from "${src}". ` +
                        'Only named document imports and graphql/gql are supported.',
                      );
                    }
                    return;
                  }

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
                    } else if (!specifierIsTypeOnly) {
                      throw specifier.buildCodeFrameError(
                        `@graphox/babel-plugin could not rewrite "${importedName}" from "${src}". ` +
                        'Run Graphox codegen and ensure the manifest includes this document.',
                      );
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

                  if (!source) {
                    throw callPath.buildCodeFrameError(
                      `@graphox/babel-plugin could not statically analyze this ${callee.node.name}() call. ` +
                      'Use a single static string/template literal so it can be resolved from the manifest.',
                    );
                  }

                  const normalizedSource = normalize(source);
                  const entry = manifest.get(normalizedSource);
                  if (!entry) {
                    throw callPath.buildCodeFrameError(
                      `@graphox/babel-plugin could not find this ${callee.node.name}() document in the manifest. ` +
                      'Run Graphox codegen and ensure the build is using the correct manifest.',
                    );
                  }

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
            },
          });

          // Ensure we never remove the entrypoint import while runtime references still exist.
          programPath.traverse({
            Identifier(idPath) {
              if (!idPath.isReferencedIdentifier()) {
                return;
              }

              const binding = idPath.scope.getBinding(idPath.node.name);
              if (!binding || !graphqlIds.has(binding)) {
                return;
              }

              throw idPath.buildCodeFrameError(
                `@graphox/babel-plugin left a runtime reference to "${idPath.node.name}" after rewriting. ` +
                'All Graphox graphql/gql imports must be fully inlined before the import is removed.',
              );
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
