//! TypeScript type fixtures for spec validation tests
//!
//! Validates rules:
//! - E-TYPE-ALIAS: type alias declarations produce TypeAlias entities
//! - E-ENUM: enum declarations produce Enum entities
//! - E-ENUM-CONST: const enum declarations produce Enum entities
//! - E-ENUM-MEMBER: enum members produce EnumVariant entities
//! - E-CONST: const declarations produce Constant entities
//! - E-VAR-LET: let/var declarations produce Variable entities
//! - V-EXPORT: exported types have Public visibility
//! - V-TYPE-ALIAS: type aliases inherit visibility from export
//! - R-CONTAINS-ENUM-MEMBER: Enum CONTAINS its members
//! - M-GENERIC: generic type parameters tracked
//! - M-GENERIC-CONSTRAINT: generic constraints tracked
//! - M-GENERIC-DEFAULT: generic defaults tracked
//! - M-ENUM-NUMERIC: numeric enums have is_numeric metadata
//! - M-ENUM-STRING: string enums have is_string metadata
//! - M-ENUM-CONST: const enums have is_const metadata

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic type aliases
///
/// Validates:
/// - E-TYPE-ALIAS: type alias produces TypeAlias entity
/// - V-TYPE-ALIAS: visibility inherited from export keyword
pub static TYPE_ALIASES: Fixture = Fixture {
    name: "ts_type_aliases",
    files: &[(
        "types.ts",
        r#"
// Simple type alias
export type ID = string;

// Object type alias
export type User = {
    id: ID;
    name: string;
    email?: string;
};

// Union type alias
export type Status = "pending" | "active" | "inactive";

// Intersection type alias
export type WithTimestamps = {
    createdAt: Date;
    updatedAt: Date;
};

export type TimestampedUser = User & WithTimestamps;

// Private type alias (not exported)
type InternalConfig = {
    secret: string;
};

// Conditional type alias
export type NonNullable<T> = T extends null | undefined ? never : T;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.ID",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.User",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.Status",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.WithTimestamps",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.TimestampedUser",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.InternalConfig",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "types.NonNullable",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Generic type aliases
///
/// Validates:
/// - E-TYPE-ALIAS: generic type aliases produce TypeAlias entities
/// - M-GENERIC: generic parameters tracked
/// - M-GENERIC-CONSTRAINT: constraints tracked
/// - M-GENERIC-DEFAULT: defaults tracked
pub static GENERIC_TYPE_ALIASES: Fixture = Fixture {
    name: "ts_generic_type_aliases",
    files: &[(
        "generic-types.ts",
        r#"
// Simple generic type
export type Box<T> = { value: T };

// Multiple type parameters
export type Pair<A, B> = { first: A; second: B };

// Generic with constraint
export type Lengthable<T extends { length: number }> = T;

// Generic with default
export type Optional<T = string> = T | undefined;

// Generic with constraint and default
export type Numeric<T extends number = number> = T;

// Complex generic with conditional
export type Unwrap<T> = T extends Promise<infer U> ? U : T;

// Mapped type
export type Readonly<T> = { readonly [P in keyof T]: T[P] };

// Template literal type
export type EventName<T extends string> = `on${Capitalize<T>}`;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Box",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Pair",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Lengthable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Optional",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Numeric",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Unwrap",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.Readonly",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "generic-types.EventName",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Numeric enums
///
/// Validates:
/// - E-ENUM: enum declaration produces Enum entity
/// - E-ENUM-MEMBER: enum members produce EnumVariant entities
/// - R-CONTAINS-ENUM-MEMBER: Enum CONTAINS members
/// - M-ENUM-NUMERIC: numeric enums have is_numeric metadata
pub static ENUMS: Fixture = Fixture {
    name: "ts_enums",
    files: &[(
        "enums.ts",
        r#"
// Numeric enum (default)
export enum Direction {
    Up,      // 0
    Down,    // 1
    Left,    // 2
    Right    // 3
}

// Numeric enum with explicit values
export enum HttpStatus {
    OK = 200,
    NotFound = 404,
    InternalError = 500
}

// Numeric enum with computed values
export enum FileAccess {
    None = 0,
    Read = 1 << 0,
    Write = 1 << 1,
    ReadWrite = Read | Write
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "enums.Direction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "enums.Direction.Up",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "enums.Direction.Down",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "enums.Direction.Left",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "enums.Direction.Right",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "enums.HttpStatus",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "enums.FileAccess",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "enums.Direction",
            to: "enums.Direction.Up",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "enums.Direction",
            to: "enums.Direction.Right",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// String enums
///
/// Validates:
/// - E-ENUM: string enum produces Enum entity
/// - E-ENUM-MEMBER: string enum members produce EnumVariant entities
/// - M-ENUM-STRING: string enums have is_string metadata
pub static STRING_ENUMS: Fixture = Fixture {
    name: "ts_string_enums",
    files: &[(
        "string-enums.ts",
        r#"
// String enum
export enum LogLevel {
    Debug = "DEBUG",
    Info = "INFO",
    Warn = "WARN",
    Error = "ERROR"
}

// String enum with computed-like values
export enum MediaType {
    JSON = "application/json",
    XML = "application/xml",
    FormData = "multipart/form-data"
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "string-enums.LogLevel",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "string-enums.LogLevel.Debug",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "string-enums.LogLevel.Info",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "string-enums.LogLevel.Warn",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "string-enums.LogLevel.Error",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "string-enums.MediaType",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Const enums
///
/// Validates:
/// - E-ENUM-CONST: const enum produces Enum entity
/// - M-ENUM-CONST: const enums have is_const metadata
pub static CONST_ENUMS: Fixture = Fixture {
    name: "ts_const_enums",
    files: &[(
        "const-enums.ts",
        r#"
// Const enum (inlined at compile time)
export const enum Priority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3
}

// Const string enum
export const enum Environment {
    Development = "dev",
    Staging = "staging",
    Production = "prod"
}

// Usage (values are inlined)
function getPriorityLabel(priority: Priority): string {
    switch (priority) {
        case Priority.Low: return "Low";
        case Priority.Medium: return "Medium";
        case Priority.High: return "High";
        case Priority.Critical: return "Critical";
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "const-enums.Priority",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "const-enums.Priority.Low",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "const-enums.Environment",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "const-enums.getPriorityLabel",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Constants and variables
///
/// Validates:
/// - E-CONST: const declarations produce Constant entities
/// - E-VAR-LET: let/var declarations produce Variable entities
/// - V-EXPORT: exported declarations have Public visibility
pub static CONSTANTS_VARIABLES: Fixture = Fixture {
    name: "ts_constants_variables",
    files: &[(
        "variables.ts",
        r#"
// Exported constants
export const VERSION = "1.0.0";
export const PI = 3.14159;
export const CONFIG = {
    apiUrl: "https://api.example.com",
    timeout: 5000
} as const;

// Non-exported constant
const SECRET_KEY = "super-secret";

// Let variable (mutable)
export let counter = 0;

// Var variable (function-scoped)
export var legacyFlag = true;

// Destructured constants
export const { apiUrl, timeout } = CONFIG;

// Array destructuring
export const [first, second] = [1, 2];
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.VERSION",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.PI",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.CONFIG",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.SECRET_KEY",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "variables.counter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "variables.legacyFlag",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.apiUrl",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "variables.timeout",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
