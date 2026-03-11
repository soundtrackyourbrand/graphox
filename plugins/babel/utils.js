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

module.exports = {
  findNearestFile,
  resolveTsConfigPaths,
  resolvePackageJsonImports
};
