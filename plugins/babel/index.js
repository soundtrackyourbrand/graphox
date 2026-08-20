const path = require('path');
const fs = require('fs');
const {
  findNearestFile,
  resolveTsConfigPaths,
  resolvePackageJsonImports,
  resolvePackageExportAlias
} = require('./utils');

// Resolution depends only on the plugin options, but Program.enter runs per
// file. Babel hands back the same options object each time, so key the cache on
// it — this also stops the exports warning repeating once per module.
const resolvedOutputsCache = new WeakMap();

/**
 * Collapse a document's source into a key that ignores formatting.
 *
 * Whitespace between GraphQL tokens carries no meaning, and dropping it is what
 * lets a call site's indentation differ from the manifest's. Inside a string or
 * block string it does carry meaning: dropping it there made two documents that
 * differ only in a literal's contents share one key, so the first entry answered
 * for both and a call site silently got the other document. Anonymous operations
 * make that reachable — there is no duplicate name to reject them.
 *
 * This is a key builder, not a GraphQL parser. It only has to be wrong in the
 * same way on both sides of a comparison.
 */
function normalize(s) {
  let out = '';
  let i = 0;

  while (i < s.length) {
    if (s[i] !== '"') {
      if (!/\s/.test(s[i])) out += s[i];
      i += 1;
      continue;
    }

    const block = s.startsWith('\"\"\"', i);
    const quote = block ? 3 : 1;

    out += '"'.repeat(quote);
    i += quote;

    while (i < s.length) {
      if (s[i] === '\\') {
        // An escape cannot close the string, so take both characters.
        out += s[i] + (s[i + 1] ?? '');
        i += 2;
        continue;
      }

      if (s[i] === '"' && (!block || s.startsWith('\"\"\"', i))) {
        out += '"'.repeat(quote);
        i += quote;
        break;
      }

      out += s[i];
      i += 1;
    }
  }

  return out;
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
          const opts = state.opts;
          const currentFile = state.file.opts.filename;
          const currentDir = currentFile ? path.dirname(currentFile) : process.cwd();

          const extension = getExtension(opts.emitExtensions);

          let outputs = resolvedOutputsCache.get(opts);
          if (!outputs) {
            const declared =
              opts.outputs && opts.outputs.length > 0
                ? opts.outputs
                : [
                    {
                      outputDir: opts.outputDir,
                      manifestPath: opts.manifestPath,
                      manifestData: opts.manifestData,
                      graphqlImportPaths: opts.graphqlImportPaths,
                      importAlias: opts.importAlias,
                      packageRoot: opts.packageRoot,
                    },
                  ];

            for (const output of declared) {
              if (!output.outputDir) {
                throw new Error('outputDir is required for @graphox/babel-plugin');
              }
            }

            const tsconfigPath = findNearestFile(currentDir, 'tsconfig.json');
            const pkgJsonPath = findNearestFile(currentDir, 'package.json');
            const rootDir =
              pkgJsonPath || tsconfigPath
                ? path.dirname(pkgJsonPath || tsconfigPath)
                : process.cwd();

            outputs = declared.map((output) => {
              const absoluteOutputDir = path.isAbsolute(output.outputDir)
                ? output.outputDir
                : path.resolve(rootDir, output.outputDir);

              let manifestFile = output.manifestPath;
              if (manifestFile && !path.isAbsolute(manifestFile)) {
                manifestFile = path.resolve(rootDir, manifestFile);
              }
              if (!manifestFile && !output.manifestData) {
                manifestFile = path.join(absoluteOutputDir, 'manifest.json');
              }

              const projectTsconfig =
                findNearestFile(absoluteOutputDir, 'tsconfig.json') || tsconfigPath;
              const projectPkgJson =
                findNearestFile(absoluteOutputDir, 'package.json') || pkgJsonPath;

              const importPathsSet = new Set(
                (output.graphqlImportPaths || []).map(toPosixPath)
              );
              const unusableAliases = [];
              for (const scan of [
                projectTsconfig
                  ? resolveTsConfigPaths(projectTsconfig, absoluteOutputDir)
                  : { paths: [], unusable: [] },
                projectPkgJson
                  ? resolvePackageJsonImports(projectPkgJson, absoluteOutputDir)
                  : { paths: [], unusable: [] },
              ]) {
                for (const p of scan.paths) {
                  importPathsSet.add(toPosixPath(p));
                }
                for (const alias of scan.unusable) {
                  if (!unusableAliases.includes(alias)) unusableAliases.push(alias);
                }
              }

              // An alias that leads here but yielded no specifier is the
              // dangerous case: the entrypoint is emptied because its path
              // matches, while call sites reaching it through that alias are
              // never rewritten and go on calling the emptied stub.
              for (const alias of unusableAliases) {
                console.warn(
                  `@graphox/babel-plugin: "${alias}" leads to ${absoluteOutputDir}, but graphox ` +
                    `could not work out which import specifier its call sites use. Their graphql() ` +
                    `calls will not be rewritten, and ${path.join(absoluteOutputDir, 'graphql')} is ` +
                    `emptied either way, so they fail at runtime. Add the specifier they import to ` +
                    `graphqlImportPaths for this output.`
                );
              }

              // A sibling file named like the output directory makes the bare
              // specifier ambiguous: `./gen` resolves to gen.ts, not gen/,
              // because a file beats a directory.
              for (const ext of ['.ts', '.tsx', '.mts', '.cts', '.js', '.jsx', '.mjs', '.cjs']) {
                if (fs.existsSync(`${absoluteOutputDir}${ext}`)) {
                  console.warn(
                    `@graphox/babel-plugin: "${absoluteOutputDir}${ext}" sits next to the output ` +
                      `directory ${absoluteOutputDir}, so an import of ` +
                      `"${path.basename(absoluteOutputDir)}" resolves to that file while graphox ` +
                      `reads it as the generated barrel. Rename one of them, or import the barrel ` +
                      `explicitly as "${path.basename(absoluteOutputDir)}/graphql".`
                  );
                  break;
                }
              }

              // Modules inside this package import its documents by relative
              // path; anything outside has to go through the alias, because a
              // relative path would reach past the package's subpath exports.
              const packageRoot =
                output.packageRoot ||
                (projectPkgJson ? path.dirname(projectPkgJson) : undefined);

              let importAlias = output.importAlias;
              if (!importAlias && projectPkgJson) {
                const inferred = resolvePackageExportAlias(projectPkgJson, absoluteOutputDir);
                if (inferred) {
                  importAlias = inferred.alias;
                  if (!inferred.canServeDeep) {
                    console.warn(
                      `@graphox/babel-plugin: "${inferred.alias}" resolves to ${absoluteOutputDir}, ` +
                        `but ${projectPkgJson} has no exports entry that can serve files inside it. ` +
                        `Documents imported from another package are rewritten to ` +
                        `"${inferred.alias}/<file>", which will not resolve. Add ` +
                        `"${inferred.subpath}/*": "${inferred.subpath}/*" to its exports.`
                    );
                  }
                }
              }

              // The alias is also how another project names this entrypoint.
              if (importAlias) {
                importPathsSet.add(toPosixPath(importAlias));
              }

              let entries = [];
              if (output.manifestData) {
                entries = output.manifestData;
              } else if (manifestFile) {
                try {
                  entries = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
                } catch (e) {
                  // Ignore a missing manifest during build
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

              return {
                outputDir: absoluteOutputDir,
                importAlias,
                packageRoot,
                graphqlImportPaths: Array.from(importPathsSet),
                entrypointPath: path.join(absoluteOutputDir, 'graphql'),
                indexPath: path.join(absoluteOutputDir, 'index'),
                manifest,
                documentNameToEntry,
              };
            });

            for (let i = 0; i < outputs.length; i++) {
              for (let j = i + 1; j < outputs.length; j++) {
                const a = outputs[i].outputDir;
                const b = outputs[j].outputDir;
                if (a === b) {
                  throw new Error(
                    `@graphox/babel-plugin: duplicate outputDir "${a}" in outputs.`
                  );
                }
                if (b.startsWith(a + path.sep) || a.startsWith(b + path.sep)) {
                  throw new Error(
                    `@graphox/babel-plugin: outputDir "${a}" and "${b}" overlap. Outputs must ` +
                      `be distinct so each module belongs to exactly one.`
                  );
                }
              }
            }

            resolvedOutputsCache.set(opts, outputs);
          }

          const unresolvedDocumentError = (importedName, source) =>
            `@graphox/babel-plugin could not rewrite "${importedName}" from "${source}". ` +
            `It is in none of the configured manifests [` +
            outputs.map((o) => o.outputDir).join(', ') +
            `]. Register the outputDir of the project that defines it, or run Graphox codegen.`;

          // If processing any configured entrypoint, clear it
          if (currentFile) {
            const currentFileNoExt = currentFile.replace(/\.(js|ts)x?$/, '');
            if (outputs.some((output) => currentFileNoExt === output.entrypointPath)) {
              programPath.node.body = [
                t.exportNamedDeclaration(
                  t.variableDeclaration('const', [
                    t.variableDeclarator(
                      t.identifier('graphql'),
                      // Nothing should reach this: a rewritten call site imports
                      // the generated document directly, and a call site still
                      // holding a live reference to `graphql` is a build error
                      // already. It is reachable only when the plugin failed to
                      // recognise the specifier some module used to import this
                      // entrypoint — that module is then untouched while the
                      // entrypoint it calls is emptied regardless, because
                      // clearing keys on the file path. Returning a non-document
                      // there fails much later, inside whichever client receives
                      // it, with nothing pointing back here.
                      t.arrowFunctionExpression(
                        [],
                        t.blockStatement([
                          t.throwStatement(
                            t.newExpression(t.identifier('Error'), [
                              t.stringLiteral(
                                `@graphox/babel-plugin: ${currentFile} was emptied at build time — ` +
                                  `its documents are inlined into the generated files — but graphql() ` +
                                  `was called through it at runtime. The plugin did not recognise the ` +
                                  `specifier the calling module used to import this entrypoint, so that ` +
                                  `module was never rewritten. Add the specifier it imports to ` +
                                  `graphqlImportPaths for this output.`
                              ),
                            ])
                          ),
                        ])
                      )
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

          // Which output's entrypoint (or index barrel) `src` refers to, if any.
          // Returns the index so callers know whose manifest to use and how to
          // write the replacement import.
          const resolveGraphqlPath = (src) => {
            src = toPosixPath(src);
            const srcNoExt = stripScriptExtension(src);

            for (let i = 0; i < outputs.length; i++) {
              const paths = outputs[i].graphqlImportPaths;
              if (paths.includes(src) || paths.includes(srcNoExt)) return i;

              // Directory aliases discovered from tsconfig/package.json
              for (const p of paths) {
                if (p.endsWith('/') && src.startsWith(p)) {
                  const subPathNoExt = stripScriptExtension(src.slice(p.length));
                  if (subPathNoExt === 'graphql' || subPathNoExt === 'index') return i;
                }
              }
            }

            if (!currentDir) return null;

            let absoluteSrc = null;
            try {
              if (src.startsWith('.') || path.isAbsolute(src)) {
                absoluteSrc = path.resolve(currentDir, srcNoExt);
              } else {
                absoluteSrc = stripScriptExtension(require.resolve(src, { paths: [currentDir] }));
              }
            } catch (_) {}

            if (!absoluteSrc) return null;

            for (let i = 0; i < outputs.length; i++) {
              const { entrypointPath, indexPath } = outputs[i];
              if (absoluteSrc === entrypointPath || absoluteSrc === indexPath) return i;
              if (path.join(absoluteSrc, 'index') === indexPath) return i;
            }

            return null;
          };

          const isOurGraphqlPath = (src) => resolveGraphqlPath(src) !== null;

          const getStaticString = (node) => {
            if (t.isStringLiteral(node)) {
              return node.value;
            }

            if (
              t.isTemplateLiteral(node) &&
              node.expressions.length === 0 &&
              node.quasis.length === 1
            ) {
              return node.quasis[0].value.cooked || node.quasis[0].value.raw;
            }

            return null;
          };

          const getImportPath = (outputIdx, entryPath) => {
            const output = outputs[outputIdx];
            const inSamePackage = output.packageRoot && currentFile
              ? currentFile.startsWith(output.packageRoot)
              : true;

            if (!inSamePackage) {
              if (!output.importAlias) {
                throw new Error(
                  `@graphox/babel-plugin: "${entryPath}" belongs to the output at ` +
                    `"${output.outputDir}", which has no importAlias. Set one so documents ` +
                    `in it can be imported from other projects.`
                );
              }
              const file = stripScriptExtension(toPosixPath(entryPath)).replace(/^\.\//, '');
              return `${output.importAlias.replace(/\/$/, '')}/${file}${extension}`;
            }

            const codegenAbsPath = path.join(output.outputDir, entryPath);
            let relPath = path.relative(path.dirname(currentFile), codegenAbsPath);
            relPath = toPosixPath(relPath);
            if (!relPath.startsWith('.') && !path.isAbsolute(relPath)) {
              relPath = './' + relPath;
            }
            return relPath + extension;
          };

          const getDynamicImportInfo = (exprPath) => {
            if (!exprPath?.node) {
              return null;
            }

            if (exprPath.isAwaitExpression()) {
              const importCallPath = exprPath.get('argument');
              if (!importCallPath.isCallExpression()) {
                return null;
              }

              if (!importCallPath.get('callee').isImport()) {
                return null;
              }

              const [sourceArgPath] = importCallPath.get('arguments');
              if (!sourceArgPath?.node) {
                return null;
              }

              return { awaited: true, sourceArgPath };
            }

            if (!exprPath.isCallExpression() || !exprPath.get('callee').isImport()) {
              return null;
            }

            const [sourceArgPath] = exprPath.get('arguments');
            if (!sourceArgPath?.node) {
              return null;
            }

            return { awaited: false, sourceArgPath };
          };

          const buildDynamicImportRewrite = (requests, scope) => {
            const moduleNameByPath = new Map();
            const uniquePaths = [];

            for (const { sourcePath } of requests) {
              if (!moduleNameByPath.has(sourcePath)) {
                moduleNameByPath.set(sourcePath, scope.generateUid('graphoxModule'));
                uniquePaths.push(sourcePath);
              }
            }

            if (uniquePaths.length === 1) {
              return t.callExpression(t.import(), [t.stringLiteral(uniquePaths[0])]);
            }

            const imports = uniquePaths.map((sourcePath) =>
              t.callExpression(t.import(), [t.stringLiteral(sourcePath)])
            );
            const moduleParams = uniquePaths.map((sourcePath) =>
              t.identifier(moduleNameByPath.get(sourcePath))
            );
            const properties = requests.map(({ importedName, sourcePath }) =>
              t.objectProperty(
                t.identifier(importedName),
                t.memberExpression(
                  t.identifier(moduleNameByPath.get(sourcePath)),
                  t.identifier(importedName),
                ),
              ),
            );

            return t.callExpression(
              t.memberExpression(
                t.callExpression(
                  t.memberExpression(t.identifier('Promise'), t.identifier('all')),
                  [t.arrayExpression(imports)],
                ),
                t.identifier('then'),
              ),
              [
                t.arrowFunctionExpression(
                  [t.arrayPattern(moduleParams)],
                  t.objectExpression(properties),
                ),
              ],
            );
          };

          // binding -> index of the output whose entrypoint it came from, so a
          // call resolves against that project's manifest.
          const graphqlIds = new Map();
          // newImports stores: localName -> { sourcePath, importedName }
          const newImports = new Map();
          // (owning output, document name) -> local name in this file. Keyed by
          // the output too: two projects may legitimately export the same
          // document name, and each needs its own binding.
          const documentNameToLocalName = new Map();
          const localNameKey = (outputIdx, documentName) => `${outputIdx}\u0000${documentName}`;

          const getLocalName = (outputIdx, documentName, scope) => {
            const key = localNameKey(outputIdx, documentName);
            if (documentNameToLocalName.has(key)) {
              return documentNameToLocalName.get(key);
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

            // A name another output's document has already claimed is taken too,
            // or the two collide in newImports and one import disappears.
            if (isColliding || newImports.has(uniqueName)) {
              uniqueName = scope.generateUid(documentName);
            }

            documentNameToLocalName.set(key, uniqueName);
            return uniqueName;
          };

          // First pass: identify imports from our graphql.ts or index.ts
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              const outputIdx = resolveGraphqlPath(src);
              if (outputIdx !== null) {
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
                      graphqlIds.set(specifier.scope.getBinding(localName), outputIdx);
                    } else if (
                      outputs[outputIdx].documentNameToEntry.has(importedName) &&
                      !specifierIsTypeOnly
                    ) {
                      // Only rewrite non-type-only imports of document names
                      const entry = outputs[outputIdx].documentNameToEntry.get(importedName);
                      const relPath = getImportPath(outputIdx, entry.path);

                      // If the original import was aliased (e.g. import { D as MyD }),
                      // we want to keep that alias if possible.
                      let targetLocalName = localName;
                      if (localName === importedName) {
                        // It was NOT aliased, check for collisions
                        targetLocalName = getLocalName(outputIdx, importedName, programPath.scope);
                      } else {
                        // It WAS aliased. We should keep the alias but ensure it's recorded
                        // so that subsequent graphql(`...`) calls use the same alias.
                        documentNameToLocalName.set(localNameKey(outputIdx, importedName), localName);
                      }

                      newImports.set(targetLocalName, { sourcePath: relPath, importedName });

                      if (localName !== targetLocalName) {
                        specifier.scope.rename(localName, targetLocalName);
                      }
                    } else if (!specifierIsTypeOnly) {
                      throw specifier.buildCodeFrameError(
                        unresolvedDocumentError(importedName, src) + ' ' +
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
                  const outputIdx = graphqlIds.get(binding);
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
                  const entry = outputs[outputIdx].manifest.get(normalizedSource);
                  if (!entry) {
                    throw callPath.buildCodeFrameError(
                      `@graphox/babel-plugin could not find this ${callee.node.name}() document in the manifest for ` +
                      `"${outputs[outputIdx].outputDir}". ` +
                      'Run Graphox codegen and ensure the build is using the correct manifest.',
                    );
                  }

                  const relPath = getImportPath(outputIdx, entry.path);

                  const uniqueLocalName = getLocalName(outputIdx, entry.name, callPath.scope);
                  newImports.set(uniqueLocalName, { sourcePath: relPath, importedName: entry.name });
                  callPath.replaceWith(t.identifier(uniqueLocalName));
                }
              }
            },
          });

          // Third pass: rewrite supported dynamic import patterns from graphql.ts/index.ts.
          programPath.traverse({
            VariableDeclarator(varPath) {
              const initPath = varPath.get('init');
              const info = getDynamicImportInfo(initPath);
              if (!info) {
                return;
              }

              const source = getStaticString(info.sourceArgPath.node);
              if (!source || !isOurGraphqlPath(source)) {
                return;
              }

              const idPath = varPath.get('id');
              if (!idPath.isObjectPattern()) {
                throw varPath.buildCodeFrameError(
                  `@graphox/babel-plugin could not fully rewrite this dynamic import from "${source}". ` +
                  'Use object destructuring of named documents from the generated graphql entrypoint or split the import by document.',
                );
              }

              const requests = [];
              for (const propertyPath of idPath.get('properties')) {
                if (!propertyPath.isObjectProperty() || propertyPath.node.computed) {
                  throw propertyPath.buildCodeFrameError(
                    `@graphox/babel-plugin could not fully rewrite this dynamic import from "${source}". ` +
                    'Use object destructuring of named documents from the generated graphql entrypoint or split the import by document.',
                  );
                }

                const key = propertyPath.node.key;
                const importedName = t.isIdentifier(key)
                  ? key.name
                  : t.isStringLiteral(key)
                    ? key.value
                    : null;

                if (!importedName || importedName === 'graphql' || importedName === 'gql') {
                  throw propertyPath.buildCodeFrameError(
                    `@graphox/babel-plugin could not fully rewrite this dynamic import from "${source}". ` +
                    'Use object destructuring of named documents from the generated graphql entrypoint or split the import by document.',
                  );
                }

                const dynamicOutputIdx = resolveGraphqlPath(source) ?? 0;
                const entry = outputs[dynamicOutputIdx].documentNameToEntry.get(importedName);
                if (!entry) {
                  throw propertyPath.buildCodeFrameError(
                    unresolvedDocumentError(importedName, source) + ' ' +
                    'Run Graphox codegen and ensure the manifest includes this document.',
                  );
                }

                requests.push({
                  importedName,
                  sourcePath: getImportPath(dynamicOutputIdx, entry.path),
                });
              }

              if (requests.length === 0) {
                return;
              }

              const replacement = buildDynamicImportRewrite(requests, varPath.scope);
              initPath.replaceWith(info.awaited ? t.awaitExpression(replacement) : replacement);
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

          // Reject runtime dynamic imports we could not rewrite before the entrypoint is cleared.
          programPath.traverse({
            CallExpression(callPath) {
              if (!callPath.get('callee').isImport()) {
                return;
              }

              const [sourceArgPath] = callPath.get('arguments');
              if (!sourceArgPath?.node) {
                return;
              }

              const source = getStaticString(sourceArgPath.node);
              if (!source || !isOurGraphqlPath(source)) {
                return;
              }

              throw callPath.buildCodeFrameError(
                `@graphox/babel-plugin could not fully rewrite this dynamic import from "${source}". ` +
                'Use object destructuring of named documents from the generated graphql entrypoint or split the import by document.',
              );
            },
          });

          // Fourth pass: redirect re-exports of documents at the generated files.
          // A re-export binds nothing locally, so only the source moves — but
          // left pointing at the entrypoint it resolves to nothing, because that
          // module is emptied in its own compilation.
          const starReexportError = (source) =>
            `@graphox/babel-plugin could not rewrite a star re-export of "${source}". ` +
            `That entrypoint is emptied at build time, so nothing would be left to re-export. ` +
            `Name the documents instead: export { SomeDocument } from "${source}".`;

          programPath.traverse({
            ExportAllDeclaration(exportPath) {
              if (isOurGraphqlPath(exportPath.node.source.value)) {
                throw exportPath.buildCodeFrameError(
                  starReexportError(exportPath.node.source.value)
                );
              }
            },
            ExportNamedDeclaration(exportPath) {
              const source = exportPath.node.source;
              if (!source || !isOurGraphqlPath(source.value)) {
                return;
              }

              const outputIdx = resolveGraphqlPath(source.value);
              const order = [];
              const byPath = new Map();

              for (const specifier of exportPath.node.specifiers) {
                // `export * as ns from` and `export v from` would both need the
                // emptied entrypoint to still hold the documents.
                if (!t.isExportSpecifier(specifier)) {
                  throw exportPath.buildCodeFrameError(starReexportError(source.value));
                }

                // Types are erased before this output runs, and the entrypoint
                // they came from is emptied, so a type-only re-export has nothing
                // left to carry. Dropped, as a type-only import from the
                // entrypoint is.
                if (exportPath.node.exportKind === 'type' || specifier.exportKind === 'type') {
                  continue;
                }

                const localName = t.isIdentifier(specifier.local)
                  ? specifier.local.name
                  : specifier.local.value;

                if (localName === 'graphql' || localName === 'gql') {
                  throw exportPath.buildCodeFrameError(
                    `@graphox/babel-plugin could not re-export "${localName}" from "${source.value}". ` +
                    'It is replaced at build time and does not exist at runtime.',
                  );
                }

                const entry = outputs[outputIdx].documentNameToEntry.get(localName);
                if (!entry) {
                  throw exportPath.buildCodeFrameError(
                    unresolvedDocumentError(localName, source.value) + ' ' +
                    'Run Graphox codegen and ensure the manifest includes this document.',
                  );
                }

                const target = getImportPath(outputIdx, entry.path);
                if (!byPath.has(target)) {
                  byPath.set(target, []);
                  order.push(target);
                }
                byPath.get(target).push(specifier);
              }

              // Documents named in one declaration can live in different
              // generated files, so a declaration may become several.
              const replacements = order.map((target) =>
                t.exportNamedDeclaration(null, byPath.get(target), t.stringLiteral(target))
              );

              if (replacements.length > 0) {
                exportPath.replaceWithMultiple(replacements);
              } else {
                exportPath.remove();
              }
            },
          });

          // Where the imports we are about to remove stood. Putting the new ones
          // at the top instead would move them ahead of a side-effect import that
          // was written to run first.
          let insertIndex = programPath.node.body.findIndex(
            (node) => t.isImportDeclaration(node) && isOurGraphqlPath(node.source.value)
          );
          if (insertIndex < 0) insertIndex = 0;

          // Fifth pass: remove ALL imports from graphql.ts/index.ts
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              if (isOurGraphqlPath(src)) {
                importPath.remove();
              }
            },
          });

          // Add the new imports where the ones they replace stood (sorted
          // alphabetically by local name)
          const sortedNewImports = Array.from(newImports.entries()).sort((a, b) => a[0].localeCompare(b[0]));
          programPath.node.body.splice(
            insertIndex,
            0,
            ...sortedNewImports.map(([localName, { sourcePath, importedName }]) =>
              t.importDeclaration(
                [t.importSpecifier(t.identifier(localName), t.identifier(importedName))],
                t.stringLiteral(sourcePath)
              )
            )
          );
        },
      },
    },
  };
};
