//! TypeScript module fixtures for spec validation tests
//!
//! Validates rules:
//! - E-MOD-FILE: TypeScript files produce Module entities
//! - E-MOD-NAMESPACE: namespace declarations produce Module entities
//! - E-MOD-NAMESPACE-MERGED: merged namespaces produce a single Module entity
//! - V-EXPORT: exported items have Public visibility
//! - V-EXPORT-DEFAULT: default exports have Public visibility
//! - V-NAMESPACE-EXPORT: namespace-exported items have Public visibility
//! - Q-MODULE-FILE: file modules use file path as qualified name
//! - Q-MODULE-NAMESPACE: namespace modules use declaration name
//! - Q-ITEM-NAMESPACE: items in namespaces are qualified under namespace path
//! - R-CONTAINS-MODULE-ITEM: Module CONTAINS its items
//! - R-IMPORTS-NAMED: named imports create IMPORTS relationships
//! - R-IMPORTS-DEFAULT: default imports create IMPORTS relationships
//! - R-IMPORTS-NAMESPACE: namespace imports create IMPORTS relationships
//! - R-IMPORTS-TYPE: type-only imports create IMPORTS relationships
//! - R-REEXPORTS-NAMED: named re-exports create REEXPORTS relationships
//! - R-REEXPORTS-ALL: star re-exports create REEXPORTS relationships
//! - R-REEXPORTS-NAMESPACE: namespace re-exports create REEXPORTS relationships

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic TypeScript module with exports
///
/// Validates:
/// - E-MOD-FILE: TypeScript file produces Module entity
/// - V-EXPORT: `export` keyword results in Public visibility
/// - Q-MODULE-FILE: module qualified name is based on file path
/// - R-CONTAINS-MODULE-ITEM: module CONTAINS exported function
pub static BASIC_MODULE: Fixture = Fixture {
    name: "ts_basic_module",
    files: &[(
        "index.ts",
        r#"
export function greet(name: string): string {
    return `Hello, ${name}!`;
}

export const VERSION = "1.0.0";

function privateHelper(): void {
    // not exported
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "index",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "index.greet",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "index.VERSION",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "index.privateHelper",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "index",
            to: "index.greet",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "index",
            to: "index.VERSION",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "index",
            to: "index.privateHelper",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Named and default imports
///
/// Validates:
/// - R-IMPORTS-NAMED: import { x } from creates IMPORTS relationship
/// - R-IMPORTS-DEFAULT: import x from creates IMPORTS relationship
/// - R-IMPORTS-NAMESPACE: import * as x from creates IMPORTS relationship
pub static IMPORTS_EXPORTS: Fixture = Fixture {
    name: "ts_imports_exports",
    files: &[
        (
            "utils.ts",
            r#"
export function helper(): void {}
export const VALUE = 42;
export default class DefaultClass {}
"#,
        ),
        (
            "consumer.ts",
            r#"
import DefaultClass, { helper, VALUE } from './utils';
import * as Utils from './utils';

export function useAll(): void {
    helper();
    console.log(VALUE);
    new DefaultClass();
    Utils.helper();
}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.helper",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "utils.VALUE",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "utils.DefaultClass",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "consumer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "consumer.useAll",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        // Named imports
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "consumer",
            to: "utils.helper",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "consumer",
            to: "utils.VALUE",
        },
        // Default import
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "consumer",
            to: "utils.DefaultClass",
        },
        // Namespace import creates import relationship to module
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "consumer",
            to: "utils",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Namespace declarations
///
/// Validates:
/// - E-MOD-NAMESPACE: namespace Foo {} produces Module entity
/// - V-NAMESPACE-EXPORT: export inside namespace has Public visibility
/// - Q-MODULE-NAMESPACE: namespace qualified name uses declaration name
/// - Q-ITEM-NAMESPACE: items qualified under namespace path
pub static NAMESPACES: Fixture = Fixture {
    name: "ts_namespaces",
    files: &[(
        "geometry.ts",
        r#"
export namespace Geometry {
    export interface Point {
        x: number;
        y: number;
    }

    export function distance(a: Point, b: Point): number {
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        return Math.sqrt(dx * dx + dy * dy);
    }

    function internalHelper(): void {
        // not exported from namespace
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "geometry",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "geometry.Geometry",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "geometry.Geometry.Point",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "geometry.Geometry.distance",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "geometry.Geometry.internalHelper",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "geometry",
            to: "geometry.Geometry",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "geometry.Geometry",
            to: "geometry.Geometry.Point",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "geometry.Geometry",
            to: "geometry.Geometry.distance",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Nested namespace declarations
///
/// Validates:
/// - E-MOD-NAMESPACE: nested namespaces produce Module entities
/// - Q-MODULE-NAMESPACE: nested namespaces use dotted path
pub static NESTED_NAMESPACES: Fixture = Fixture {
    name: "ts_nested_namespaces",
    files: &[(
        "shapes.ts",
        r#"
export namespace Shapes {
    export namespace TwoD {
        export interface Circle {
            radius: number;
        }
    }

    export namespace ThreeD {
        export interface Sphere {
            radius: number;
        }
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "shapes.Shapes",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "shapes.Shapes.TwoD",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "shapes.Shapes.ThreeD",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "shapes.Shapes.TwoD.Circle",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "shapes.Shapes.ThreeD.Sphere",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "shapes.Shapes",
            to: "shapes.Shapes.TwoD",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "shapes.Shapes",
            to: "shapes.Shapes.ThreeD",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Namespace declaration merging
///
/// Validates:
/// - E-MOD-NAMESPACE-MERGED: multiple declarations with same name merge into one entity
pub static NAMESPACE_MERGING: Fixture = Fixture {
    name: "ts_namespace_merging",
    files: &[(
        "animal.ts",
        r#"
export namespace Animal {
    export interface Dog {
        breed: string;
    }
}

// This merges with the above namespace
export namespace Animal {
    export interface Cat {
        color: string;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "animal.Animal",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "animal.Animal.Dog",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "animal.Animal.Cat",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animal.Animal",
            to: "animal.Animal.Dog",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animal.Animal",
            to: "animal.Animal.Cat",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Named and star re-exports
///
/// Validates:
/// - R-REEXPORTS-NAMED: export { x } from creates REEXPORTS relationship
/// - R-REEXPORTS-ALL: export * from creates REEXPORTS relationships
/// - R-REEXPORTS-NAMESPACE: export * as ns from creates REEXPORTS relationship
pub static REEXPORTS: Fixture = Fixture {
    name: "ts_reexports",
    files: &[
        (
            "internal.ts",
            r#"
export function internalFn(): void {}
export const INTERNAL_VALUE = 1;
"#,
        ),
        (
            "index.ts",
            r#"
// Named re-export
export { internalFn } from './internal';

// Re-export with rename
export { INTERNAL_VALUE as VALUE } from './internal';

// Star re-export (all exports from module)
export * from './internal';

// Namespace re-export
export * as Internal from './internal';
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "internal",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "internal.internalFn",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "internal.INTERNAL_VALUE",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test-package",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        // Named re-export
        ExpectedRelationship {
            kind: RelationshipKind::Reexports,
            from: "test-package",
            to: "internal.internalFn",
        },
        // Renamed re-export
        ExpectedRelationship {
            kind: RelationshipKind::Reexports,
            from: "test-package",
            to: "internal.INTERNAL_VALUE",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Barrel exports (index.ts re-exporting from multiple modules)
///
/// Validates:
/// - R-REEXPORTS-ALL: barrel pattern with star exports
pub static BARREL_EXPORTS: Fixture = Fixture {
    name: "ts_barrel_exports",
    files: &[
        (
            "models/user.ts",
            r#"
export interface User {
    id: string;
    name: string;
}
"#,
        ),
        (
            "models/post.ts",
            r#"
export interface Post {
    id: string;
    title: string;
}
"#,
        ),
        (
            "models/index.ts",
            r#"
export * from './user';
export * from './post';
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "models.user",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "models.user.User",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "models.post",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "models.post.Post",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "models",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Reexports,
            from: "models",
            to: "models.user.User",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Reexports,
            from: "models",
            to: "models.post.Post",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Default exports
///
/// Validates:
/// - V-EXPORT-DEFAULT: default exported items have Public visibility
/// - R-IMPORTS-DEFAULT: default imports create IMPORTS relationships
pub static DEFAULT_EXPORTS: Fixture = Fixture {
    name: "ts_default_exports",
    files: &[
        (
            "app.ts",
            r#"
export default class App {
    run(): void {
        console.log("Running");
    }
}
"#,
        ),
        (
            "main.ts",
            r#"
import App from './app';

const app = new App();
app.run();
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "app",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "app.App",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "app.App.run",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "main",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Imports,
        from: "main",
        to: "app.App",
    }],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
