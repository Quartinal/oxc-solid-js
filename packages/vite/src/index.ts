import type { Plugin, FilterPattern } from 'vite';
import { createFilter } from 'vite';
import { createRequire } from 'node:module';
import { dirname, isAbsolute, resolve as resolvePath } from 'node:path';
import { readFileSync } from 'node:fs';

export interface SolidOxcOptions {
  /**
   * Filter which files to transform
   * @default /\.[jt]sx$/
   */
  include?: FilterPattern;

  /**
   * Filter which files to exclude
   * @default /node_modules/
   */
  exclude?: FilterPattern;

  /**
   * The module to import runtime helpers from
   * @default '@solidjs/web'
   */
  module_name?: string;

  /**
   * Generate mode
   * @default 'dom'
   */
  generate?: 'dom' | 'ssr' | 'universal';

  /**
   * Enable hydration support
   * @default false
   */
  hydratable?: boolean;

  /**
   * Delegate events for better performance
   * @default true
   */
  delegate_events?: boolean;

  /**
   * Wrap conditionals in memos
   * @default true
   */
  wrap_conditionals?: boolean;

  /**
   * Pass context to custom elements
   * @default true
   */
  context_to_custom_elements?: boolean;

  /**
   * Built-in components that should be passed through
   */
  builtIns?: string[];

  /**
   * Enable SSR mode (shorthand for generate: 'ssr')
   * @default false
   */
  ssr?: boolean;

  /**
   * Dev mode - enables additional debugging
   * @default based on vite mode
   */
  dev?: boolean;

  /**
   * Hot module replacement
   * @default true in dev mode
   */
  hot?: boolean;

  /**
   * Add Vite resolve condition `solid` (resolves JSX sources in dependencies).
   * Disabled by default so dependencies resolve to precompiled JS unless you opt in.
   * @default false
   */
  solid_condition?: boolean;
}

const defaultOptions: SolidOxcOptions = {
  include: /\.[jt]sx$/,
  exclude: /node_modules/,
  module_name: '@solidjs/web',
  generate: 'dom',
  hydratable: false,
  delegate_events: true,
  wrap_conditionals: true,
  context_to_custom_elements: true,
  builtIns: [
    'For',
    'Show',
    'Switch',
    'Match',
    'Loading',
    'Reveal',
    'Portal',
    'Dynamic',
    'Errored',
  ],
};

const BARE_IMPORT_RE = /^[^./]|^\.[^./]|^\.\.[^/]/;
const EXPORT_CONDITION_PREFERENCE = ['solid', 'default', 'development', 'production'] as const;

function parsePackageSpecifier(source: string): { packageName: string; subpath: string } | null {
  if (!BARE_IMPORT_RE.test(source)) {
    return null;
  }

  if (source.startsWith('@')) {
    const [scope, name, ...rest] = source.split('/');
    if (!scope || !name) return null;
    return {
      packageName: `${scope}/${name}`,
      subpath: rest.length > 0 ? `./${rest.join('/')}` : '.',
    };
  }

  const [name, ...rest] = source.split('/');
  if (!name) return null;
  return {
    packageName: name,
    subpath: rest.length > 0 ? `./${rest.join('/')}` : '.',
  };
}

function pickExportTarget(entry: unknown): string | null {
  if (typeof entry === 'string') {
    return entry;
  }

  if (!entry || typeof entry !== 'object') {
    return null;
  }

  const record = entry as Record<string, unknown>;
  for (const key of EXPORT_CONDITION_PREFERENCE) {
    if (key in record) {
      const picked = pickExportTarget(record[key]);
      if (picked) return picked;
    }
  }

  for (const value of Object.values(record)) {
    const picked = pickExportTarget(value);
    if (picked) return picked;
  }

  return null;
}

/**
 * Vite plugin for SolidJS using OXC-based compiler
 */
export default function solidOxc(options: SolidOxcOptions = {}): Plugin {
  const opts = { ...defaultOptions, ...options };
  const filter = createFilter(opts.include, opts.exclude);
  const packageJsonCache = new Map<string, unknown>();

  let isDev = false;
  let needHmr = false;
  let runtimeCode: string | null = null;
  const REFRESH_VIRTUAL_ID = '/@solid-refresh';

  // Lazy load the native module
  let oxcJsxDomExpr: typeof import('@oxc-solid-js/compiler') | null = null;

  return {
    name: '@oxc-solid-js/vite',
    sharedDuringBuild: false,

    enforce: 'pre',

    configEnvironment(_name, config) {
      if (!opts.solid_condition) {
        return;
      }

      const conditions = config.resolve?.conditions ?? [];
      if (conditions.includes('solid')) {
        return;
      }

      return {
        resolve: {
          conditions: ['solid', ...conditions],
        },
      };
    },

    configResolved(config) {
      isDev = config.command === 'serve';

      // Determine if HMR should be active
      needHmr = isDev && opts.hot !== false;
      if (needHmr) {
        try {
          const req = createRequire(import.meta.url);
          const runtimeFilePath = req.resolve('solid-refresh/dist/solid-refresh.mjs');
          runtimeCode = readFileSync(runtimeFilePath, 'utf-8');
        } catch {
          console.warn(
            '[@oxc-solid-js/vite] solid-refresh not found, HMR disabled. Install with: pnpm add solid-refresh'
          );
          needHmr = false;
        }
      }
    },

    async buildStart() {
      // Load the native module
      try {
        oxcJsxDomExpr = await import('@oxc-solid-js/compiler');
      } catch (e) {
        this.error(
          'Failed to load @oxc-solid-js/compiler. Ensure the package is installed and a platform binary is available.\n' +
          'The compiler loader can use GitHub Release OS tarballs automatically when published.'
        );
      }
    },

    async resolveId(source, importer, options) {
      // Serve solid-refresh runtime as virtual module
      if (source === REFRESH_VIRTUAL_ID || source === 'solid-refresh') {
        return REFRESH_VIRTUAL_ID;
      }

      if (!opts.solid_condition) {
        return null;
      }

      if (!options?.ssr || this.environment?.config?.consumer !== 'server') {
        return null;
      }

      if (!BARE_IMPORT_RE.test(source)) {
        return null;
      }

      const ssrResolved = await this.resolve(source, importer, { ...options, skipSelf: true });
      if (ssrResolved && ssrResolved.id !== source && !ssrResolved.external) {
        return null;
      }

      const parsed = parsePackageSpecifier(source);
      if (!parsed) {
        return null;
      }

      const resolverBase =
        importer && (isAbsolute(importer) || importer.startsWith('file://'))
          ? importer
          : resolvePath(process.cwd(), '__vite_plugin_solid_oxc__.js');
      const resolver = createRequire(resolverBase);

      let packageJsonPath: string;
      try {
        packageJsonPath = resolver.resolve(`${parsed.packageName}/package.json`);
      } catch {
        return null;
      }

      let packageJson = packageJsonCache.get(packageJsonPath);
      if (!packageJson) {
        try {
          packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8')) as unknown;
          packageJsonCache.set(packageJsonPath, packageJson);
        } catch {
          return null;
        }
      }

      const exportsField =
        packageJson && typeof packageJson === 'object' && 'exports' in packageJson
          ? (packageJson as { exports: unknown }).exports
          : undefined;
      if (!exportsField) {
        return null;
      }

      let exportEntry: unknown;
      if (parsed.subpath === '.') {
        exportEntry =
          typeof exportsField === 'object' && exportsField !== null && '.' in (exportsField as Record<string, unknown>)
            ? (exportsField as Record<string, unknown>)['.']
            : exportsField;
      } else {
        exportEntry =
          typeof exportsField === 'object' && exportsField !== null
            ? (exportsField as Record<string, unknown>)[parsed.subpath]
            : undefined;
      }

      const solidTarget = pickExportTarget(exportEntry);
      if (!solidTarget) {
        return null;
      }

      const normalized = solidTarget.replace(/^\.\/+/, '');
      const resolvedId = resolvePath(dirname(packageJsonPath), normalized);

      if (
        !resolvedId.includes('/dist/source/') &&
        !resolvedId.endsWith('.jsx') &&
        !resolvedId.endsWith('.tsx')
      ) {
        return null;
      }

      return { id: resolvedId };
    },

    load(id) {
      if (id === REFRESH_VIRTUAL_ID) {
        return runtimeCode;
      }
    },

    async transform(code, id, transformOptions) {
      const fileId = id.split('?', 1)[0];

      if (!filter(fileId)) {
        return null;
      }

      if (!oxcJsxDomExpr) {
        this.error('@oxc-solid-js/compiler module not loaded');
        return null;
      }

      const transformSSR =
        opts.ssr ??
        (typeof transformOptions?.ssr === 'boolean' ? transformOptions.ssr : false);
      const generate = transformSSR ? 'ssr' : opts.generate;
      const moduleName =
        transformSSR && opts.module_name === defaultOptions.module_name
          ? 'solid-js/web'
          : opts.module_name;

      try {
        const result = oxcJsxDomExpr.transformJsx(code, {
          filename: fileId,
          moduleName,
          generate,
          hydratable: opts.hydratable,
          delegateEvents: opts.delegate_events,
          wrapConditionals: opts.wrap_conditionals,
          contextToCustomElements: opts.context_to_custom_elements,
          sourceMap: true,
          // HMR options: active in dev, client-side only
          ...(needHmr && !transformSSR ? {
            hmr: true,
            hmrBundler: 'vite',
            hmrGranular: true,
            hmrJsx: true,
            hmrFixRender: true,
          } : {}),
        });

        // NOTE: No naive HMR shim here — the Rust solid-refresh transform
        // already injects proper import.meta.hot.accept() + $$refresh() calls

        return {
          code: result.code,
          map: result.map ? JSON.parse(result.map) : null,
        };
      } catch (e: unknown) {
        const message = e instanceof Error ? e.message : String(e);
        this.error(`Failed to transform ${id}: ${message}`);
        return null;
      }
    },

    // Handle Solid's JSX types
    config(_userConfig, { command }) {
      // Pre-compute HMR need based on command (config runs before configResolved)
      const willNeedHmr = command === 'serve' && opts.hot !== false;

      const resolveConditions: string[] | undefined = opts.solid_condition
        ? ['solid']
        : undefined;
      return {
        oxc: {
          // Let our plugin handle JSX
          jsx: 'preserve',
          jsxImportSource: 'solid-js',
        },
        resolve: {
          ...(resolveConditions ? { conditions: resolveConditions } : {}),
          dedupe: ['solid-js', '@solidjs/web'],
          alias: willNeedHmr ? [{ find: /^solid-refresh$/, replacement: REFRESH_VIRTUAL_ID }] : [],
        },
      };
    },
  };
}

// Named export for compatibility
export { solidOxc };

// Type exports
export type { Plugin };
