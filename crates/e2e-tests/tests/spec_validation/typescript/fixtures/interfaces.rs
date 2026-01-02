//! TypeScript interface fixtures for spec validation tests
//!
//! Validates rules:
//! - E-INTERFACE: interface declarations produce Interface entities
//! - E-INTERFACE-MERGED: multiple interface declarations merge into one entity
//! - E-PROPERTY-INTERFACE: interface property signatures produce Property entities
//! - E-INDEX-SIGNATURE: index signatures produce Property entities
//! - E-CALL-SIGNATURE: call signatures produce Method entities
//! - E-CONSTRUCT-SIGNATURE: construct signatures produce Method entities
//! - V-INTERFACE-MEMBER: interface members have Public visibility (always)
//! - R-CONTAINS-INTERFACE-MEMBER: Interface CONTAINS its members
//! - R-EXTENDS-INTERFACE: interface EXTENDS_INTERFACE another interface

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic interface with properties and methods
///
/// Validates:
/// - E-INTERFACE: interface declaration produces Interface entity
/// - E-PROPERTY-INTERFACE: property signatures produce Property entities
/// - V-INTERFACE-MEMBER: all interface members are Public
/// - R-CONTAINS-INTERFACE-MEMBER: Interface CONTAINS members
pub static INTERFACES: Fixture = Fixture {
    name: "ts_interfaces",
    files: &[(
        "user.ts",
        r#"
export interface User {
    id: number;
    name: string;
    email?: string;
    readonly createdAt: Date;

    greet(): string;
    updateEmail(email: string): void;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "user.User",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.id",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.email",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.createdAt",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "user.User.greet",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "user.User.updateEmail",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "user.User",
            to: "user.User.id",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "user.User",
            to: "user.User.greet",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Interface extending other interfaces
///
/// Validates:
/// - E-INTERFACE: all interfaces produce Interface entities
/// - R-EXTENDS-INTERFACE: subinterface EXTENDS_INTERFACE superinterface
pub static INTERFACE_EXTENDS: Fixture = Fixture {
    name: "ts_interface_extends",
    files: &[(
        "entities.ts",
        r#"
export interface Entity {
    id: string;
}

export interface Timestamped {
    createdAt: Date;
    updatedAt: Date;
}

export interface Auditable extends Entity, Timestamped {
    createdBy: string;
    updatedBy: string;
}

export interface SoftDeletable extends Entity {
    deletedAt?: Date;
}

// Deep inheritance chain
export interface AuditableAndDeletable extends Auditable, SoftDeletable {
    version: number;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "entities.Entity",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "entities.Timestamped",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "entities.Auditable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "entities.SoftDeletable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "entities.AuditableAndDeletable",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "entities.Auditable",
            to: "entities.Entity",
        },
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "entities.Auditable",
            to: "entities.Timestamped",
        },
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "entities.SoftDeletable",
            to: "entities.Entity",
        },
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "entities.AuditableAndDeletable",
            to: "entities.Auditable",
        },
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "entities.AuditableAndDeletable",
            to: "entities.SoftDeletable",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Interface declaration merging
///
/// Validates:
/// - E-INTERFACE-MERGED: multiple declarations with same name produce single Interface entity
pub static INTERFACE_MERGING: Fixture = Fixture {
    name: "ts_interface_merging",
    files: &[(
        "window.ts",
        r#"
// First declaration
export interface Window {
    title: string;
}

// Merged declaration (adds more members)
export interface Window {
    width: number;
    height: number;
}

// Third merged declaration
export interface Window {
    resize(w: number, h: number): void;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "window.Window",
            visibility: Some(Visibility::Public),
        },
        // All properties from merged declarations
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "window.Window.title",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "window.Window.width",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "window.Window.height",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "window.Window.resize",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "window.Window",
            to: "window.Window.title",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "window.Window",
            to: "window.Window.resize",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Index signatures in interfaces
///
/// Validates:
/// - E-INDEX-SIGNATURE: index signatures produce Property entities with special names
pub static INDEX_SIGNATURES: Fixture = Fixture {
    name: "ts_index_signatures",
    files: &[(
        "dictionary.ts",
        r#"
export interface StringDictionary {
    [key: string]: string;
}

export interface NumberDictionary {
    [index: number]: string;
}

export interface MixedDictionary {
    // Named property
    name: string;
    // String index signature
    [key: string]: string | number;
    // Numeric index (must be subtype of string index)
}

export interface ReadonlyDictionary {
    readonly [key: string]: number;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "dictionary.StringDictionary",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "dictionary.StringDictionary.[string]",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "dictionary.NumberDictionary",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "dictionary.NumberDictionary.[number]",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "dictionary.MixedDictionary",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "dictionary.ReadonlyDictionary",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Call and construct signatures
///
/// Validates:
/// - E-CALL-SIGNATURE: call signatures produce Method entities
/// - E-CONSTRUCT-SIGNATURE: construct signatures produce Method entities
pub static CALL_CONSTRUCT_SIGNATURES: Fixture = Fixture {
    name: "ts_call_construct_signatures",
    files: &[(
        "callable.ts",
        r#"
// Interface with call signature (callable)
export interface Callable {
    (x: number): number;
}

// Interface with multiple call signatures (overloaded)
export interface OverloadedCallable {
    (x: number): number;
    (x: string): string;
}

// Interface with construct signature (newable)
export interface Constructable {
    new (name: string): object;
}

// Interface with both call and construct signatures
export interface CallableAndConstructable {
    (x: number): number;
    new (name: string): object;
}

// Interface with call signature and properties
export interface FunctionWithProperties {
    (x: number): number;
    displayName: string;
    version: number;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "callable.Callable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "callable.Callable.()",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "callable.OverloadedCallable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "callable.Constructable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "callable.Constructable.new()",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "callable.CallableAndConstructable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "callable.FunctionWithProperties",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "callable.FunctionWithProperties.displayName",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
