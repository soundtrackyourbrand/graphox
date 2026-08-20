const fs = require('fs');
const path = require('path');
const jsonc = require('jsonc-parser');

const stripExt = (p) => p.replace(/(\.d)?\.(mjs|cjs|js|jsx|ts|tsx)$/, '');

function findNearestFile(startDir, filename) {
  let currentDir = startDir;
  while (currentDir) {
    const filePath = path.join(currentDir, filename);
    if (fs.existsSync(filePath)) {
      return filePath;
    }
    const parentDir = path.dirname(currentDir);
    if (parentDir === currentDir) {
      break;
    }
    currentDir = parentDir;
  }
  return null;
}

function resolveTsConfigPaths(tsconfigPath, absoluteOutputDir) {
  try {
    const content = fs.readFileSync(tsconfigPath, 'utf8');
    const tsconfig = jsonc.parse(content);
    const paths = tsconfig?.compilerOptions?.paths;
    if (!paths) return [];

    const baseUrl = tsconfig.compilerOptions.baseUrl 
      ? path.resolve(path.dirname(tsconfigPath), tsconfig.compilerOptions.baseUrl) 
      : path.dirname(tsconfigPath);
    
    const matchedPaths = [];
    const absoluteEntrypointPath = path.join(absoluteOutputDir, 'graphql');
    const absoluteIndexPath = path.join(absoluteOutputDir, 'index');

    const absOutNoExt = stripExt(absoluteOutputDir);
    const absEntryNoExt = stripExt(absoluteEntrypointPath);
    const absIndexNoExt = stripExt(absoluteIndexPath);

    for (const [alias, targets] of Object.entries(paths)) {
      for (const target of targets) {
        const cleanTarget = target.replace(/\*$/, '');
        const absTarget = path.resolve(baseUrl, cleanTarget);
        const absTargetNoExt = stripExt(absTarget);

        if (absTargetNoExt === absOutNoExt) {
           let a = alias.replace(/\*$/, '');
           if (!a.endsWith('/')) a += '/';
           matchedPaths.push(a);
           continue;
        }

        if (absTargetNoExt === absEntryNoExt || 
            absTargetNoExt === absIndexNoExt ||
            stripExt(path.join(absTargetNoExt, 'index')) === absIndexNoExt) {
           matchedPaths.push(alias.replace(/\*$/, ''));
        }
      }
    }
    return matchedPaths;
  } catch (e) {
    console.debug(`[resolveTsConfigPaths] Error processing ${tsconfigPath}: ${e.message}`, e.stack);
    return [];
  }
}

function resolvePackageJsonImports(pkgJsonPath, absoluteOutputDir) {
  try {
    const content = fs.readFileSync(pkgJsonPath, 'utf8');
    const pkg = JSON.parse(content);
    const imports = pkg.imports;
    if (!imports) return [];

    const matchedImports = [];
    const pkgDir = path.dirname(pkgJsonPath);
    const absoluteEntrypointPath = path.join(absoluteOutputDir, 'graphql');
    const absoluteIndexPath = path.join(absoluteOutputDir, 'index');

    const absOutNoExt = stripExt(absoluteOutputDir);
    const absEntryNoExt = stripExt(absoluteEntrypointPath);
    const absIndexNoExt = stripExt(absoluteIndexPath);
    
    function checkTarget(target, alias) {
       if (typeof target !== 'string') return;
       const absTarget = path.resolve(pkgDir, target);
       const absTargetNoExt = stripExt(absTarget);

       if (absTargetNoExt === absOutNoExt) {
         let a = alias.replace(/\*$/, '');
         if (!a.endsWith('/')) a += '/';
         matchedImports.push(a);
         return;
       }

       if (absTargetNoExt === absEntryNoExt || 
           absTargetNoExt === absIndexNoExt ||
           stripExt(path.join(absTargetNoExt, 'index')) === absIndexNoExt) {
         matchedImports.push(alias.replace(/\*$/, ''));
       }
    }


    for (const [alias, target] of Object.entries(imports)) {
      if (typeof target === 'string') {
        checkTarget(target, alias);
      } else if (typeof target === 'object' && target !== null) {
        for (const key of ['import', 'types', 'default', 'require']) {
          if (target[key]) {
             checkTarget(target[key], alias);
          }
        }
      }
    }
    
    return matchedImports;
  } catch (e) {
    console.debug(`[resolvePackageJsonImports] Error processing ${pkgJsonPath}: ${e.message}`, e.stack);
    return [];
  }
}

/**
 * The bare specifier prefix other packages use to reach `absoluteOutputDir`,
 * derived from the owning package's `name` and `exports`.
 *
 * `canServeDeep` reports whether the exports map can also serve files inside
 * that subpath, which cross-project document imports need.
 *
 * Conservative by design: only reports an alias when exactly one subpath
 * matches, and skips array and null targets rather than guessing. A wrong guess
 * yields a specifier that fails to resolve at bundle time, which is worse than
 * asking for explicit config.
 */
function resolvePackageExportAlias(pkgJsonPath, absoluteOutputDir) {
  try {
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'));
    if (!pkg.name || !pkg.exports || typeof pkg.exports !== 'object') return null;

    const pkgDir = path.dirname(pkgJsonPath);
    const outNoExt = stripExt(absoluteOutputDir);
    const entryNoExt = stripExt(path.join(absoluteOutputDir, 'graphql'));
    const indexNoExt = stripExt(path.join(absoluteOutputDir, 'index'));

    const targetsOf = (target) => {
      if (typeof target === 'string') return [target];
      if (target && typeof target === 'object' && !Array.isArray(target)) {
        return ['import', 'types', 'default', 'require']
          .map((key) => target[key])
          .filter((value) => typeof value === 'string');
      }
      return [];
    };

    const matches = [];
    let canServeDeep = false;

    for (const [subpath, target] of Object.entries(pkg.exports)) {
      if (!subpath.startsWith('.')) continue;

      for (const raw of targetsOf(target)) {
        if (subpath.includes('*')) {
          const prefix = stripExt(path.resolve(pkgDir, raw.split('*')[0]));
          if (prefix === outNoExt || prefix === path.join(outNoExt, path.sep)) {
            canServeDeep = true;
          }
          continue;
        }

        const absNoExt = stripExt(path.resolve(pkgDir, raw));
        if (
          absNoExt === entryNoExt ||
          absNoExt === indexNoExt ||
          absNoExt === outNoExt ||
          stripExt(path.join(absNoExt, 'index')) === indexNoExt
        ) {
          matches.push(subpath);
        }
      }
    }

    const unique = Array.from(new Set(matches));
    if (unique.length !== 1) return null;

    const subpath = unique[0];
    const alias = subpath === '.' ? pkg.name : `${pkg.name}/${subpath.replace(/^\.\//, '')}`;
    return { alias, subpath, canServeDeep };
  } catch (e) {
    return null;
  }
}

module.exports = {
  findNearestFile,
  resolvePackageExportAlias,
  resolveTsConfigPaths,
  resolvePackageJsonImports
};
