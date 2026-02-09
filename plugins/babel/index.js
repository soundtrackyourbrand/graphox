const path = require('path');
const fs = require('fs');

function normalize(s) {
  return s.replace(/\s+/g, '');
}

module.exports = function (babel) {
  const { types: t } = babel;

  return {
    name: '@soundtrack/graphox-babel',
    visitor: {
      Program: {
        enter(programPath, state) {
          const {
            manifestPath,
            manifestData,
            outputDir,
            graphqlImportPaths = [],
          } = state.opts;

          if (!outputDir) {
            throw new Error('outputDir is required for @soundtrack/graphox-babel');
          }

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
          for (const entry of entries) {
            manifest.set(normalize(entry.source), entry);
          }

          const currentFile = state.file.opts.filename;
          const absoluteOutputDir = path.resolve(outputDir);
          const absoluteEntrypointPath = path.join(absoluteOutputDir, 'graphql');

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
            // 1. Check explicit config paths
            if (graphqlImportPaths.includes(src)) return true;
            const srcNoExt = src.replace(/\.(js|ts)x?$/, '');
            if (graphqlImportPaths.includes(srcNoExt)) return true;

            // 2. Check for subpath imports starting with # if they contain 'graphql'
            if (src.startsWith('#') && src.includes('graphql')) return true;

            // 3. Fallback to relative path detection
            if (currentFile && (src.startsWith('.') || src.startsWith('/'))) {
              const absoluteSrc = path.resolve(path.dirname(currentFile), srcNoExt);
              return absoluteSrc === absoluteEntrypointPath;
            }

            return false;
          };

          const graphqlIds = new Set();
          const newImports = new Map(); // localName -> sourcePath

          // First pass: identify imports
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              if (isOurGraphqlPath(src)) {
                importPath.get('specifiers').forEach((specifier) => {
                  if (specifier.isImportSpecifier()) {
                    const importedName = specifier.node.imported.name;
                    if (importedName === 'graphql' || importedName === 'gql') {
                      graphqlIds.add(specifier.scope.getBinding(specifier.node.local.name));
                    }
                  }
                });
              }
            },
          });

          // Second pass: transform calls
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

                      newImports.set(entry.name, relPath);
                      callPath.replaceWith(t.identifier(entry.name));
                    }
                  }
                }
              }
            },
          });

          // Third pass: remove imports and add new ones
          programPath.traverse({
            ImportDeclaration(importPath) {
              const src = importPath.node.source.value;
              if (isOurGraphqlPath(src)) {
                const specifiers = importPath.get('specifiers');
                specifiers.forEach((specifier) => {
                  if (specifier.isImportSpecifier()) {
                    const binding = specifier.scope.getBinding(specifier.node.local.name);
                    if (graphqlIds.has(binding)) {
                      specifier.remove();
                    }
                  }
                });

                if (importPath.node.specifiers.length === 0) {
                  importPath.remove();
                }
              }
            },
          });

          // Add new imports at the top
          const sortedNewImports = Array.from(newImports.entries()).sort((a, b) => a[0].localeCompare(b[0]));
          for (const [localName, sourcePath] of sortedNewImports) {
            programPath.node.body.unshift(
              t.importDeclaration(
                [t.importSpecifier(t.identifier(localName), t.identifier(localName))],
                t.stringLiteral(sourcePath)
              )
            );
          }
        },
      },
    },
  };
};
