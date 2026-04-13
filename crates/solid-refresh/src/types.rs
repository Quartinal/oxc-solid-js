use indexmap::IndexMap;

/// Bundler runtime type — determines HMR glue shape.
/// Matches JS: 'esm' | 'vite' | 'standard' | 'webpack5' | 'rspack-esm'
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeType {
    Esm,
    Vite,
    Standard,
    Webpack5,
    RspackEsm,
}

impl RuntimeType {
    /// Returns the string representation matching the JS bundler option.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Esm => "esm",
            Self::Vite => "vite",
            Self::Standard => "standard",
            Self::Webpack5 => "webpack5",
            Self::RspackEsm => "rspack-esm",
        }
    }
}

/// Import definition — named or default import from a specific source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportDefinition {
    Named {
        name: &'static str,
        source: &'static str,
    },
    Default {
        source: &'static str,
    },
}

impl ImportDefinition {
    /// Returns the source module path.
    pub fn source(&self) -> &'static str {
        match self {
            Self::Named { source, .. } | Self::Default { source } => source,
        }
    }

    /// Returns the import name ("default" for default imports).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Named { name, .. } => name,
            Self::Default { .. } => "default",
        }
    }
}

/// What kind of import identifier this tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportIdentifierType {
    Render,
    CreateContext,
}

/// A tracked import specifier (e.g., `render` from `solid-js/web`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIdentifierSpecifier {
    pub import_type: ImportIdentifierType,
    pub definition: ImportDefinition,
}

/// User-supplied options for the solid-refresh transform.
pub struct Options {
    pub granular: bool,
    pub jsx: bool,
    pub bundler: RuntimeType,
    pub fix_render: bool,
    /// Additional `createContext` import definitions from user config.
    pub extra_create_context: Vec<ImportDefinition>,
    /// Additional `render` import definitions from user config.
    pub extra_render: Vec<ImportDefinition>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            granular: true,
            jsx: true,
            bundler: RuntimeType::Standard,
            fix_render: true,
            extra_create_context: Vec::new(),
            extra_render: Vec::new(),
        }
    }
}

/// Mutable transform state threaded through all phases.
pub struct StateContext<'a> {
    pub jsx: bool,
    pub granular: bool,
    pub bundler: RuntimeType,
    pub fix_render: bool,
    pub filename: Option<&'a str>,

    /// Clone of IMPORT_SPECIFIERS + user-supplied extras.
    pub specifiers: Vec<ImportIdentifierSpecifier>,

    /// Lazily-created import identifiers, keyed by "source[name]".
    /// Value is the local identifier name (e.g., "_$$registry").
    pub imports: IndexMap<String, String>,

    /// Tracks which local import identifiers map to which specifier.
    /// Key: local binding name from ImportDeclaration.
    pub identifier_registrations: IndexMap<String, ImportIdentifierSpecifier>,

    /// Tracks namespace imports (e.g., `import * as S from 'solid-js'`).
    /// Key: local namespace name → Vec of specifiers that source provides.
    pub namespace_registrations: IndexMap<String, Vec<ImportIdentifierSpecifier>>,

    /// Counter for generating unique identifiers.
    pub uid_counter: u32,

    /// The local name for the $$registry import (set by create_registry).
    pub registry_import_name: Option<String>,
    /// The local name for the $$refresh import (set by create_registry).
    pub refresh_import_name: Option<String>,
}

impl<'a> StateContext<'a> {
    /// Creates a new StateContext from Options.
    pub fn new(options: &Options, filename: Option<&'a str>) -> Self {
        use crate::constants::IMPORT_SPECIFIERS;

        let mut specifiers: Vec<ImportIdentifierSpecifier> = IMPORT_SPECIFIERS.to_vec();

        // Append user-supplied extra import specifiers
        for def in &options.extra_render {
            specifiers.push(ImportIdentifierSpecifier {
                import_type: ImportIdentifierType::Render,
                definition: def.clone(),
            });
        }
        for def in &options.extra_create_context {
            specifiers.push(ImportIdentifierSpecifier {
                import_type: ImportIdentifierType::CreateContext,
                definition: def.clone(),
            });
        }

        Self {
            jsx: options.jsx,
            granular: options.granular,
            bundler: options.bundler,
            fix_render: options.fix_render,
            filename,
            specifiers,
            imports: IndexMap::new(),
            identifier_registrations: IndexMap::new(),
            namespace_registrations: IndexMap::new(),
            uid_counter: 0,
            registry_import_name: None,
            refresh_import_name: None,
        }
    }

    /// Generates a unique identifier name with the given prefix.
    /// Produces names like `_prefix`, `_prefix2`, `_prefix3`, ...
    pub fn generate_uid(&mut self, prefix: &str) -> String {
        self.uid_counter += 1;
        if self.uid_counter == 1 {
            format!("_{prefix}")
        } else {
            format!("_{prefix}{}", self.uid_counter)
        }
    }
}
