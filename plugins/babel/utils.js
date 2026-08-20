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

/**
 * The import specifiers one alias mapping gives this output's entrypoint.
 *
 * The mapping may be a wildcard with text on both sides of the `*` —
 * `"#gql/*": "./gen/*.ts"` is the ordinary shape for a TypeScript subpath map.
 * Resolving that target verbatim leaves an `*` inside the path, which can never
 * equal a concrete file, so solve for what `*` has to stand for to land on the
 * entrypoint and substitute that back into the alias.
 *
 * `reachesOutput` reports that the mapping leads into the output directory, so
 * the caller can tell "not this output's alias" apart from "this output's alias,
 * in a shape we could not read".
 */
function aliasSpecifiers(alias, target, baseDir, absoluteOutputDir) {
  const absOutNoExt = stripExt(absoluteOutputDir);
  const absEntryNoExt = stripExt(path.join(absoluteOutputDir, 'graphql'));
  const absIndexNoExt = stripExt(path.join(absoluteOutputDir, 'index'));

  if (!alias.includes('*') || !target.includes('*')) {
    const absTargetNoExt = stripExt(path.resolve(baseDir, target));

    // Names the directory itself, so the entrypoint is one segment further in.
    if (absTargetNoExt === absOutNoExt) {
      const prefix = alias.endsWith('/') ? alias : `${alias}/`;
      return { specifiers: [`${prefix}graphql`, `${prefix}index`], reachesOutput: true };
    }

    if (
      absTargetNoExt === absEntryNoExt ||
      absTargetNoExt === absIndexNoExt ||
      stripExt(path.join(absTargetNoExt, 'index')) === absIndexNoExt
    ) {
      return { specifiers: [alias], reachesOutput: true };
    }

    return { specifiers: [], reachesOutput: false };
  }

  const absPattern = path.resolve(baseDir, stripExt(target));
  const star = absPattern.indexOf('*');
  if (star === -1) return { specifiers: [], reachesOutput: false };

  const prefix = absPattern.slice(0, star);
  const suffix = absPattern.slice(star + 1);
  const reachesOutput = `${absOutNoExt}${path.sep}`.startsWith(prefix);

  const specifiers = [];
  for (const candidate of [absEntryNoExt, absIndexNoExt]) {
    if (candidate.length <= prefix.length + suffix.length) continue;
    if (!candidate.startsWith(prefix) || !candidate.endsWith(suffix)) continue;
    const substitution = candidate
      .slice(prefix.length, candidate.length - suffix.length)
      .split(path.sep)
      .join('/');
    specifiers.push(alias.replace('*', substitution));
  }

  return { specifiers, reachesOutput };
}

function resolveTsConfigPaths(tsconfigPath, absoluteOutputDir) {
  try {
    const content = fs.readFileSync(tsconfigPath, 'utf8');
    const tsconfig = jsonc.parse(content);
    const paths = tsconfig?.compilerOptions?.paths;
    if (!paths) return { paths: [], unusable: [] };

    const baseUrl = tsconfig.compilerOptions.baseUrl
      ? path.resolve(path.dirname(tsconfigPath), tsconfig.compilerOptions.baseUrl)
      : path.dirname(tsconfigPath);

    const scan = { paths: [], unusable: [] };
    for (const [alias, targets] of Object.entries(paths)) {
      for (const target of targets) {
        if (typeof target !== 'string') continue;
        const { specifiers, reachesOutput } = aliasSpecifiers(
          alias,
          target,
          baseUrl,
          absoluteOutputDir
        );
        scan.paths.push(...specifiers);
        if (reachesOutput && specifiers.length === 0) scan.unusable.push(alias);
      }
    }
    return scan;
  } catch (e) {
    console.debug(`[resolveTsConfigPaths] Error processing ${tsconfigPath}: ${e.message}`);
    return { paths: [], unusable: [] };
  }
}

function resolvePackageJsonImports(pkgJsonPath, absoluteOutputDir) {
  try {
    const content = fs.readFileSync(pkgJsonPath, 'utf8');
    const pkg = JSON.parse(content);
    const imports = pkg.imports;
    if (!imports) return { paths: [], unusable: [] };

    const pkgDir = path.dirname(pkgJsonPath);
    const scan = { paths: [], unusable: [] };

    // Every target the alias can resolve to, through any condition or fallback
    // array. Unlike picking an alias to emit, recognising an entrypoint wants to
    // be permissive: an extra specifier that never occurs costs nothing, while a
    // missed one leaves that project's call sites unrewritten.
    const targetsOf = (target) => {
      if (typeof target === 'string') return [target];
      if (Array.isArray(target)) return target.flatMap(targetsOf);
      if (target && typeof target === 'object') return Object.values(target).flatMap(targetsOf);
      return [];
    };

    for (const [alias, target] of Object.entries(imports)) {
      let matched = false;
      let reaches = false;

      for (const raw of targetsOf(target)) {
        const { specifiers, reachesOutput } = aliasSpecifiers(
          alias,
          raw,
          pkgDir,
          absoluteOutputDir
        );
        scan.paths.push(...specifiers);
        matched = matched || specifiers.length > 0;
        reaches = reaches || reachesOutput;
      }

      if (reaches && !matched) scan.unusable.push(alias);
    }

    return scan;
  } catch (e) {
    console.debug(`[resolvePackageJsonImports] Error processing ${pkgJsonPath}: ${e.message}`);
    return { paths: [], unusable: [] };
  }
}

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
