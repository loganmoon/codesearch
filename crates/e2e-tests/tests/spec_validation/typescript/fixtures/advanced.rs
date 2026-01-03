//! TypeScript advanced features fixtures for spec validation tests
//!
//! Validates rules:
//! - E-AMBIENT-VAR: declare var/let/const produces ambient Variable/Constant entities
//! - E-AMBIENT-FUNCTION: declare function produces ambient Function entity
//! - E-AMBIENT-CLASS: declare class produces ambient Class entity
//! - E-AMBIENT-MODULE: declare module produces ambient Module entity
//! - E-AMBIENT-NAMESPACE: declare namespace produces ambient Module entity
//! - E-GLOBAL-AUGMENTATION: declare global block produces global augmentation
//! - E-JSX-COMPONENT: function returning JSX produces Function entity with is_component
//! - E-JSX-ARROW-COMPONENT: arrow function returning JSX produces Function entity
//! - E-PROPERTY-ARROW-FIELD: arrow function class fields produce Property entities
//! - M-GENERIC: generic parameters tracked
//! - M-GENERIC-CONSTRAINT: generic constraints tracked
//! - M-GENERIC-DEFAULT: generic defaults tracked
//! - M-DECORATOR: decorators tracked
//! - M-DECORATOR-PARAM: decorator parameters tracked
//! - M-PROPERTY-OPTIONAL: optional properties tracked
//! - M-PROPERTY-READONLY: readonly properties tracked
//! - R-USES-TYPE: type references create USES relationships
//! - V-CLASS-PUBLIC: public class members
//! - V-CLASS-PRIVATE: private class members
//! - V-CLASS-PROTECTED: protected class members
//! - V-MODULE-PRIVATE: non-exported module members

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Generic classes and functions
///
/// Validates:
/// - M-GENERIC: generic type parameters tracked
/// - M-GENERIC-CONSTRAINT: constraints on generics tracked
/// - M-GENERIC-DEFAULT: default type parameters tracked
pub static GENERICS: Fixture = Fixture {
    name: "ts_generics",
    files: &[(
        "generics.ts",
        r#"
// Generic class
export class Container<T> {
    constructor(private value: T) {}

    getValue(): T {
        return this.value;
    }
}

// Generic with constraint
export class Comparable<T extends { compareTo(other: T): number }> {
    constructor(private items: T[]) {}

    sort(): T[] {
        return this.items.sort((a, b) => a.compareTo(b));
    }
}

// Generic with default
export class Collection<T = unknown> {
    private items: T[] = [];

    add(item: T): void {
        this.items.push(item);
    }
}

// Multiple type parameters with constraints and defaults
export class Repository<
    T extends { id: string },
    K = T["id"]
> {
    private store = new Map<K, T>();

    save(entity: T): void {
        this.store.set(entity.id as K, entity);
    }
}

// Generic function
export function identity<T>(value: T): T {
    return value;
}

// Generic with multiple constraints
export function merge<T extends object, U extends object>(a: T, b: U): T & U {
    return { ...a, ...b };
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "generics.Container",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "generics.Comparable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "generics.Collection",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "generics.Repository",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "generics.identity",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "generics.merge",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Decorators (experimental)
///
/// Validates:
/// - M-DECORATOR: decorators tracked on classes/methods/properties
/// - M-DECORATOR-PARAM: decorator arguments tracked
pub static DECORATORS: Fixture = Fixture {
    name: "ts_decorators",
    files: &[(
        "decorators.ts",
        r#"
// Class decorator
function sealed(constructor: Function) {
    Object.seal(constructor);
    Object.seal(constructor.prototype);
}

// Method decorator
function log(target: any, key: string, descriptor: PropertyDescriptor) {
    const original = descriptor.value;
    descriptor.value = function(...args: any[]) {
        console.log(`Calling ${key} with`, args);
        return original.apply(this, args);
    };
}

// Property decorator
function required(target: any, key: string) {
    // Validation logic
}

// Parameter decorator
function validate(target: any, key: string, index: number) {
    // Validation logic
}

// Decorated class
@sealed
export class UserService {
    @required
    private apiKey: string;

    constructor(@validate apiKey: string) {
        this.apiKey = apiKey;
    }

    @log
    getUser(id: string): object {
        return { id, name: "Test" };
    }
}

// Decorator with arguments
function Component(options: { selector: string }) {
    return function(constructor: Function) {
        // Component registration
    };
}

@Component({ selector: 'app-root' })
export class AppComponent {
    title = 'My App';
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "decorators.sealed",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "decorators.log",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "decorators.UserService",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "decorators.UserService.getUser",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "decorators.Component",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "decorators.AppComponent",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Ambient declarations (declare keyword)
///
/// Validates:
/// - E-AMBIENT-VAR: declare var produces ambient Variable entity
/// - E-AMBIENT-FUNCTION: declare function produces ambient Function entity
/// - E-AMBIENT-CLASS: declare class produces ambient Class entity
pub static AMBIENT_DECLARATIONS: Fixture = Fixture {
    name: "ts_ambient_declarations",
    files: &[(
        "ambient.d.ts",
        r#"
// Ambient variable declarations
declare const VERSION: string;
declare let debug: boolean;
declare var legacyGlobal: any;

// Ambient function declaration
declare function getEnv(key: string): string | undefined;

// Ambient class declaration
declare class ExternalLibrary {
    constructor(options: object);
    init(): Promise<void>;
    destroy(): void;
}

// Ambient interface (no declare needed)
interface GlobalConfig {
    apiUrl: string;
    timeout: number;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "ambient.VERSION",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "ambient.debug",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "ambient.legacyGlobal",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "ambient.getEnv",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "ambient.ExternalLibrary",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "ambient.GlobalConfig",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Global augmentation
///
/// Validates:
/// - E-GLOBAL-AUGMENTATION: declare global blocks produce augmentation entities
pub static GLOBAL_AUGMENTATION: Fixture = Fixture {
    name: "ts_global_augmentation",
    files: &[(
        "augment.ts",
        r#"
// Augment global Array
declare global {
    interface Array<T> {
        first(): T | undefined;
        last(): T | undefined;
    }
}

// Implementation
Array.prototype.first = function<T>(this: T[]): T | undefined {
    return this[0];
};

Array.prototype.last = function<T>(this: T[]): T | undefined {
    return this[this.length - 1];
};

// Augment global Window
declare global {
    interface Window {
        analytics: {
            track(event: string, data?: object): void;
        };
    }
}

export {};
"#,
    )],
    entities: &[ExpectedEntity {
        kind: EntityKind::Module,
        qualified_name: "augment",
        visibility: Some(Visibility::Public),
    }],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Type usage relationships
///
/// Validates:
/// - R-USES-TYPE: type references create USES relationships
pub static TYPE_USAGE: Fixture = Fixture {
    name: "ts_type_usage",
    files: &[(
        "models.ts",
        r#"
export interface User {
    id: string;
    name: string;
}

export interface Post {
    id: string;
    title: string;
    author: User;  // Uses User
}

export class UserRepository {
    private users: User[] = [];  // Uses User

    getById(id: string): User | undefined {  // Uses User
        return this.users.find(u => u.id === id);
    }

    create(user: User): void {  // Uses User
        this.users.push(user);
    }
}

// Function using types
export function formatPost(post: Post): string {  // Uses Post
    return `${post.title} by ${post.author.name}`;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "models.User",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "models.Post",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "models.UserRepository",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "models.formatPost",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "models.Post",
            to: "models.User",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "models.UserRepository",
            to: "models.User",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "models.formatPost",
            to: "models.Post",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Visibility modifiers
///
/// Validates:
/// - V-CLASS-PUBLIC: public members have Public visibility
/// - V-CLASS-PRIVATE: private members have Private visibility
/// - V-CLASS-PROTECTED: protected members have Protected visibility
/// - V-MODULE-PRIVATE: non-exported items have Private visibility
pub static VISIBILITY: Fixture = Fixture {
    name: "ts_visibility",
    files: &[(
        "visibility.ts",
        r#"
// Module-level visibility
export const publicConst = "public";
const privateConst = "private";

export function publicFunction(): void {}
function privateFunction(): void {}

// Class member visibility
export class MyClass {
    // Default (public in TS)
    defaultProp: string = "";

    // Explicit public
    public publicProp: string = "";

    // Private
    private privateProp: string = "";

    // Protected
    protected protectedProp: string = "";

    // Methods with visibility
    public publicMethod(): void {}
    private privateMethod(): void {}
    protected protectedMethod(): void {}
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "visibility.publicConst",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "visibility.privateConst",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "visibility.publicFunction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "visibility.privateFunction",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "visibility.MyClass",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "visibility.MyClass.publicProp",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "visibility.MyClass.privateProp",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "visibility.MyClass.protectedProp",
            visibility: Some(Visibility::Protected),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "visibility.MyClass.publicMethod",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "visibility.MyClass.privateMethod",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "visibility.MyClass.protectedMethod",
            visibility: Some(Visibility::Protected),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// JSX components (React-style)
///
/// Validates:
/// - E-JSX-COMPONENT: function returning JSX produces Function with is_component
/// - E-JSX-ARROW-COMPONENT: arrow function returning JSX produces Function
pub static JSX_COMPONENTS: Fixture = Fixture {
    name: "ts_jsx_components",
    files: &[(
        "components.tsx",
        r#"
import React from 'react';

// Function component
export function Greeting({ name }: { name: string }) {
    return <div>Hello, {name}!</div>;
}

// Arrow function component
export const Button = ({ onClick, children }: {
    onClick: () => void;
    children: React.ReactNode;
}) => {
    return <button onClick={onClick}>{children}</button>;
};

// Typed with React.FC
export const Card: React.FC<{ title: string }> = ({ title, children }) => {
    return (
        <div className="card">
            <h2>{title}</h2>
            {children}
        </div>
    );
};

// Component with generic props
export function List<T>({ items, renderItem }: {
    items: T[];
    renderItem: (item: T) => React.ReactNode;
}) {
    return <ul>{items.map(renderItem)}</ul>;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "components.Greeting",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "components.Button",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "components.Card",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "components.List",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Arrow function field properties
///
/// Validates:
/// - E-PROPERTY-ARROW-FIELD: arrow function class fields produce Property entities
pub static ARROW_FIELD_PROPERTIES: Fixture = Fixture {
    name: "ts_arrow_field_properties",
    files: &[(
        "handlers.ts",
        r#"
export class EventEmitter {
    // Arrow function fields (auto-bound)
    onClick = (event: MouseEvent): void => {
        console.log("Clicked", event);
    };

    onHover = (event: MouseEvent): void => {
        console.log("Hovered", event);
    };

    // Generic arrow field
    transform = <T>(value: T): T => {
        return value;
    };

    // Arrow field with complex type
    processItems = (items: string[]): string[] => {
        return items.map(item => item.toUpperCase());
    };
}

// Arrow fields in object literals are different
export const handlers = {
    click: (e: Event) => console.log(e),
    hover: (e: Event) => console.log(e),
};
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "handlers.EventEmitter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "handlers.EventEmitter.onClick",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "handlers.EventEmitter.onHover",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "handlers.EventEmitter.transform",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "handlers.EventEmitter.processItems",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Optional and readonly properties
///
/// Validates:
/// - M-PROPERTY-OPTIONAL: optional properties have is_optional metadata
/// - M-PROPERTY-READONLY: readonly properties have is_readonly metadata
pub static OPTIONAL_READONLY: Fixture = Fixture {
    name: "ts_optional_readonly",
    files: &[(
        "config.ts",
        r#"
export interface Config {
    // Required property
    name: string;

    // Optional property
    description?: string;

    // Readonly property
    readonly version: string;

    // Optional and readonly
    readonly createdAt?: Date;
}

export class Settings {
    // Required class property
    public theme: string = "light";

    // Optional class property
    public language?: string;

    // Readonly class property
    public readonly id: string;

    // Optional readonly
    public readonly lastModified?: Date;

    constructor(id: string) {
        this.id = id;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "config.Config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.description",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.version",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "config.Settings",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Settings.theme",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Settings.language",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Settings.id",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
