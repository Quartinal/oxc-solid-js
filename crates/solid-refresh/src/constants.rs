use crate::types::{ImportDefinition, ImportIdentifierSpecifier, ImportIdentifierType};

pub const SOLID_REFRESH_MODULE: &str = "solid-refresh";

pub const IMPORT_REGISTRY: ImportDefinition = ImportDefinition::Named {
    name: "$$registry",
    source: SOLID_REFRESH_MODULE,
};

pub const IMPORT_REFRESH: ImportDefinition = ImportDefinition::Named {
    name: "$$refresh",
    source: SOLID_REFRESH_MODULE,
};

pub const IMPORT_COMPONENT: ImportDefinition = ImportDefinition::Named {
    name: "$$component",
    source: SOLID_REFRESH_MODULE,
};

pub const IMPORT_CONTEXT: ImportDefinition = ImportDefinition::Named {
    name: "$$context",
    source: SOLID_REFRESH_MODULE,
};

pub const IMPORT_DECLINE: ImportDefinition = ImportDefinition::Named {
    name: "$$decline",
    source: SOLID_REFRESH_MODULE,
};

/// Default import specifiers to track in user code.
pub const IMPORT_SPECIFIERS: &[ImportIdentifierSpecifier] = &[
    ImportIdentifierSpecifier {
        import_type: ImportIdentifierType::Render,
        definition: ImportDefinition::Named {
            name: "render",
            source: "solid-js/web",
        },
    },
    ImportIdentifierSpecifier {
        import_type: ImportIdentifierType::Render,
        definition: ImportDefinition::Named {
            name: "hydrate",
            source: "solid-js/web",
        },
    },
    ImportIdentifierSpecifier {
        import_type: ImportIdentifierType::CreateContext,
        definition: ImportDefinition::Named {
            name: "createContext",
            source: "solid-js",
        },
    },
    ImportIdentifierSpecifier {
        import_type: ImportIdentifierType::CreateContext,
        definition: ImportDefinition::Named {
            name: "createContext",
            source: "solid-js/web",
        },
    },
];
