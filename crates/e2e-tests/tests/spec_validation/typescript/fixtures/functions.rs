//! TypeScript function fixtures for spec validation tests
//!
//! Validates rules:
//! - E-FN-DECL: function declarations produce Function entities
//! - E-FN-EXPR: function expressions produce Function entities
//! - E-FN-ARROW: arrow functions produce Function entities
//! - E-FN-GENERATOR: generator functions produce Function entities
//! - E-METHOD-CLASS: class methods produce Method entities
//! - V-EXPORT: exported functions have Public visibility
//! - V-MODULE-PRIVATE: unexported functions have Private visibility
//! - Q-ITEM-MODULE: functions are qualified under their module
//! - Q-METHOD-INSTANCE: instance methods use Class::method format
//! - R-CALLS-FUNCTION: function calls create CALLS relationships
//! - M-FN-ASYNC: async functions have is_async metadata
//! - M-FN-ARROW: arrow functions have is_arrow metadata
//! - M-FN-GENERATOR: generator functions have is_generator metadata
//! - M-FN-OVERLOADED: overloaded functions track overload_count

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic function declarations
///
/// Validates:
/// - E-FN-DECL: function declaration produces Function entity
/// - V-EXPORT: exported function has Public visibility
/// - V-MODULE-PRIVATE: non-exported function has Private visibility
/// - Q-ITEM-MODULE: function qualified under module
pub static FUNCTIONS: Fixture = Fixture {
    name: "ts_functions",
    files: &[(
        "utils.ts",
        r#"
export function add(a: number, b: number): number {
    return a + b;
}

export function greet(name: string): string {
    return `Hello, ${name}!`;
}

function privateHelper(): void {
    console.log("I'm private");
}

// Function with optional parameters
export function createUser(name: string, age?: number): object {
    return { name, age };
}

// Function with rest parameters
export function sum(...numbers: number[]): number {
    return numbers.reduce((a, b) => a + b, 0);
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::utils::add",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::utils::greet",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::utils::privateHelper",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::utils::createUser",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::utils::sum",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Function expressions (named and anonymous)
///
/// Validates:
/// - E-FN-EXPR: function expressions produce Function entities
pub static FUNCTION_EXPRESSIONS: Fixture = Fixture {
    name: "ts_function_expressions",
    files: &[(
        "handlers.ts",
        r#"
// Named function expression
export const onClick = function handleClick(event: Event): void {
    console.log("Clicked", event);
};

// Anonymous function expression
export const onHover = function(event: Event): void {
    console.log("Hovered", event);
};

// IIFE (Immediately Invoked Function Expression)
const result = (function initialize(): number {
    return 42;
})();
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::handlers::handleClick",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::handlers::onHover",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::handlers::initialize",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Arrow functions
///
/// Validates:
/// - E-FN-ARROW: arrow functions produce Function entities
/// - M-FN-ARROW: arrow functions have is_arrow metadata
pub static ARROW_FUNCTIONS: Fixture = Fixture {
    name: "ts_arrow_functions",
    files: &[(
        "arrows.ts",
        r#"
// Arrow function with block body
export const square = (x: number): number => {
    return x * x;
};

// Arrow function with expression body
export const double = (x: number): number => x * 2;

// Arrow function with no parameters
export const getTimestamp = (): number => Date.now();

// Arrow function with multiple parameters
export const multiply = (a: number, b: number): number => a * b;

// Arrow function with destructuring
export const getX = ({ x }: { x: number }): number => x;

// Generic arrow function
export const identity = <T>(value: T): T => value;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::square",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::double",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::getTimestamp",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::multiply",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::getX",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::arrows::identity",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Async functions
///
/// Validates:
/// - E-FN-DECL: async functions produce Function entities
/// - M-FN-ASYNC: async functions have is_async metadata
pub static ASYNC_FUNCTIONS: Fixture = Fixture {
    name: "ts_async_functions",
    files: &[(
        "api.ts",
        r#"
// Async function declaration
export async function fetchData(url: string): Promise<string> {
    const response = await fetch(url);
    return response.text();
}

// Async arrow function
export const fetchJson = async <T>(url: string): Promise<T> => {
    const response = await fetch(url);
    return response.json();
};

// Async function with error handling
export async function safelyFetch(url: string): Promise<string | null> {
    try {
        return await fetchData(url);
    } catch {
        return null;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::api::fetchData",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::api::fetchJson",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::api::safelyFetch",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test-package::api::safelyFetch",
            to: "test-package::api::fetchData",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Generator functions
///
/// Validates:
/// - E-FN-GENERATOR: generator functions produce Function entities
/// - M-FN-GENERATOR: generator functions have is_generator metadata
pub static GENERATOR_FUNCTIONS: Fixture = Fixture {
    name: "ts_generator_functions",
    files: &[(
        "generators.ts",
        r#"
// Generator function
export function* range(start: number, end: number): Generator<number> {
    for (let i = start; i < end; i++) {
        yield i;
    }
}

// Generator function with return value
export function* countdown(from: number): Generator<number, string, unknown> {
    while (from > 0) {
        yield from--;
    }
    return "Done!";
}

// Async generator function
export async function* asyncRange(start: number, end: number): AsyncGenerator<number> {
    for (let i = start; i < end; i++) {
        await new Promise(resolve => setTimeout(resolve, 100));
        yield i;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::generators::range",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::generators::countdown",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::generators::asyncRange",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Overloaded functions
///
/// Validates:
/// - E-FN-DECL: overloaded functions produce single Function entity
/// - M-FN-OVERLOADED: overloaded functions have overload_count metadata
pub static OVERLOADED_FUNCTIONS: Fixture = Fixture {
    name: "ts_overloaded_functions",
    files: &[(
        "overloads.ts",
        r#"
// Function overloads
export function format(value: number): string;
export function format(value: string): string;
export function format(value: Date): string;
export function format(value: number | string | Date): string {
    if (typeof value === "number") {
        return value.toFixed(2);
    } else if (typeof value === "string") {
        return value.toUpperCase();
    } else {
        return value.toISOString();
    }
}

// Another overloaded function
export function createElement(tag: "div"): HTMLDivElement;
export function createElement(tag: "span"): HTMLSpanElement;
export function createElement(tag: string): HTMLElement;
export function createElement(tag: string): HTMLElement {
    return document.createElement(tag);
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::overloads::format",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::overloads::createElement",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Class methods
///
/// Validates:
/// - E-METHOD-CLASS: class methods produce Method entities
/// - V-CLASS-PUBLIC: public methods have Public visibility
/// - Q-METHOD-INSTANCE: instance methods use Class::method format
pub static METHODS: Fixture = Fixture {
    name: "ts_methods",
    files: &[(
        "calculator.ts",
        r#"
export class Calculator {
    private value: number = 0;

    // Instance methods
    add(x: number): this {
        this.value += x;
        return this;
    }

    subtract(x: number): this {
        this.value -= x;
        return this;
    }

    getResult(): number {
        return this.value;
    }

    // Private method
    private reset(): void {
        this.value = 0;
    }

    // Protected method
    protected log(message: string): void {
        console.log(message);
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "test-package::calculator::Calculator",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test-package::calculator::Calculator::add",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test-package::calculator::Calculator::subtract",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test-package::calculator::Calculator::getResult",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test-package::calculator::Calculator::reset",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test-package::calculator::Calculator::log",
            visibility: Some(Visibility::Protected),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test-package::calculator::Calculator",
            to: "test-package::calculator::Calculator::add",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Function calls
///
/// Validates:
/// - R-CALLS-FUNCTION: function calls create CALLS relationships
pub static FUNCTION_CALLS: Fixture = Fixture {
    name: "ts_function_calls",
    files: &[(
        "pipeline.ts",
        r#"
export function step1(): number {
    return 1;
}

export function step2(value: number): number {
    return value * 2;
}

export function step3(value: number): string {
    return `Result: ${value}`;
}

// Function that calls other functions
export function runPipeline(): string {
    const a = step1();
    const b = step2(a);
    return step3(b);
}

// Higher-order function usage
export function applyTwice<T>(fn: (x: T) => T, value: T): T {
    return fn(fn(value));
}

export function quadruple(x: number): number {
    return applyTwice(step2, x);
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::step1",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::step2",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::step3",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::runPipeline",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::applyTwice",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test-package::pipeline::quadruple",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test-package::pipeline::runPipeline",
            to: "test-package::pipeline::step1",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test-package::pipeline::runPipeline",
            to: "test-package::pipeline::step2",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test-package::pipeline::runPipeline",
            to: "test-package::pipeline::step3",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test-package::pipeline::quadruple",
            to: "test-package::pipeline::applyTwice",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
