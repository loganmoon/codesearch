//! JavaScript fixture definitions for spec validation tests
//!
//! These fixtures validate JavaScript-specific entity extraction,
//! ensuring JavaScript handlers produce correct Language::JavaScript entities.

use codesearch_core::entities::Visibility;
use codesearch_e2e_tests::common::spec_validation::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

/// Basic JavaScript module with exports
///
/// Validates:
/// - E-MOD-FILE: JavaScript file produces Module entity
/// - V-EXPORT: `export` keyword results in Public visibility
/// - Functions and constants are extracted correctly
pub static BASIC_MODULE: Fixture = Fixture {
    name: "js_basic_module",
    files: &[(
        "index.js",
        r#"
export function greet(name) {
    return `Hello, ${name}!`;
}

export const VERSION = "1.0.0";

function privateHelper() {
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
    project_type: ProjectType::NodePackage,
    manifest: Some(
        r#"{
  "name": "js-test",
  "version": "1.0.0",
  "type": "module"
}"#,
    ),
};

/// JavaScript class with inheritance
///
/// Validates:
/// - E-CLASS: Class declarations are extracted
/// - R-INHERITS-FROM: extends relationship is captured
/// - E-METHOD: Methods inside classes are extracted
pub static CLASSES: Fixture = Fixture {
    name: "js_classes",
    files: &[(
        "animals.js",
        r#"
export class Animal {
    constructor(name) {
        this.name = name;
    }

    speak() {
        console.log(`${this.name} makes a sound`);
    }
}

export class Dog extends Animal {
    constructor(name, breed) {
        super(name);
        this.breed = breed;
    }

    speak() {
        console.log(`${this.name} barks`);
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "animals",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "animals.Animal",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "animals.Animal.constructor",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "animals.Animal.speak",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "animals.Dog",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "animals.Dog.constructor",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "animals.Dog.speak",
            visibility: None,
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals",
            to: "animals.Animal",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals",
            to: "animals.Dog",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals.Animal",
            to: "animals.Animal.constructor",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals.Animal",
            to: "animals.Animal.speak",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals.Dog",
            to: "animals.Dog.constructor",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "animals.Dog",
            to: "animals.Dog.speak",
        },
        ExpectedRelationship {
            kind: RelationshipKind::InheritsFrom,
            from: "animals.Dog",
            to: "animals.Animal",
        },
    ],
    project_type: ProjectType::NodePackage,
    manifest: Some(
        r#"{
  "name": "js-test",
  "version": "1.0.0",
  "type": "module"
}"#,
    ),
};

/// JavaScript functions with various patterns
///
/// Validates:
/// - E-FN-DECLARATION: function declarations are extracted
/// - E-FN-ARROW: arrow functions assigned to variables are extracted
/// - E-FN-EXPRESSION: function expressions are extracted
/// - M-FN-ASYNC: async functions have correct metadata
/// - M-FN-GENERATOR: generator functions have correct metadata
pub static FUNCTIONS: Fixture = Fixture {
    name: "js_functions",
    files: &[(
        "utils.js",
        r#"
export function regularFunction() {
    return 42;
}

export async function asyncFunction() {
    return await Promise.resolve(42);
}

export function* generatorFunction() {
    yield 1;
    yield 2;
}

export const arrowFunction = () => {
    return 42;
};

export const namedExpression = function named() {
    return 42;
};
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.regularFunction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.asyncFunction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.generatorFunction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.arrowFunction",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "utils.named",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "utils",
            to: "utils.regularFunction",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "utils",
            to: "utils.asyncFunction",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "utils",
            to: "utils.generatorFunction",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "utils",
            to: "utils.arrowFunction",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "utils",
            to: "utils.named",
        },
    ],
    project_type: ProjectType::NodePackage,
    manifest: Some(
        r#"{
  "name": "js-test",
  "version": "1.0.0",
  "type": "module"
}"#,
    ),
};

/// JavaScript variables (const, let, var)
///
/// Validates:
/// - E-CONST: const declarations produce Constant entities
/// - E-VAR-LET: let declarations produce Variable entities
/// - E-VAR-VAR: var declarations produce Variable entities
pub static VARIABLES: Fixture = Fixture {
    name: "js_variables",
    files: &[(
        "config.js",
        r#"
export const API_URL = "https://api.example.com";
export const MAX_RETRIES = 3;

export let currentUser = null;
export let sessionToken = "";

export var legacyFlag = true;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "config.API_URL",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "config.MAX_RETRIES",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "config.currentUser",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "config.sessionToken",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Variable,
            qualified_name: "config.legacyFlag",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config",
            to: "config.API_URL",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config",
            to: "config.MAX_RETRIES",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config",
            to: "config.currentUser",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config",
            to: "config.sessionToken",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config",
            to: "config.legacyFlag",
        },
    ],
    project_type: ProjectType::NodePackage,
    manifest: Some(
        r#"{
  "name": "js-test",
  "version": "1.0.0",
  "type": "module"
}"#,
    ),
};
