//! TypeScript class fixtures for spec validation tests
//!
//! Validates rules:
//! - E-CLASS: class declarations produce Class entities
//! - E-CLASS-ABSTRACT: abstract classes produce Class entities with is_abstract
//! - E-CLASS-EXPR: class expressions produce Class entities
//! - E-METHOD-CLASS: class methods produce Method entities
//! - E-METHOD-STATIC: static methods produce Method entities with is_static
//! - E-METHOD-ABSTRACT: abstract methods produce Method entities with is_abstract
//! - E-METHOD-GETTER: getter methods produce Method entities
//! - E-METHOD-SETTER: setter methods produce Method entities
//! - E-PROPERTY-FIELD: class fields produce Property entities
//! - E-PROPERTY-PARAM: constructor parameter properties produce Property entities
//! - E-PROPERTY-PRIVATE-FIELD: #private fields produce Property entities
//! - V-CLASS-PUBLIC: public class members have Public visibility
//! - V-CLASS-PRIVATE: private class members have Private visibility
//! - V-CLASS-PROTECTED: protected class members have Protected visibility
//! - V-CLASS-PRIVATE-FIELD: #private fields have Private visibility
//! - Q-ITEM-MODULE: classes are qualified under their module
//! - Q-METHOD-INSTANCE: instance methods use Class::method format
//! - Q-METHOD-STATIC: static methods use Class.method format
//! - Q-PROPERTY-INSTANCE: instance properties use Class::property format
//! - Q-PROPERTY-STATIC: static properties use Class.property format
//! - R-CONTAINS-CLASS-MEMBER: Class CONTAINS its members
//! - R-INHERITS-FROM: subclass INHERITS_FROM superclass
//! - R-IMPLEMENTS: class IMPLEMENTS interface
//! - M-CLASS-ABSTRACT: abstract classes have is_abstract metadata
//! - M-METHOD-STATIC: static methods have is_static metadata
//! - M-METHOD-ACCESSOR-GET: getter methods have is_accessor metadata
//! - M-METHOD-ACCESSOR-SET: setter methods have is_accessor metadata
//! - M-FIELD-INITIALIZER: fields with initializers track initialization

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic class with methods
///
/// Validates:
/// - E-CLASS: class declaration produces Class entity
/// - E-METHOD-CLASS: class methods produce Method entities
/// - V-CLASS-PUBLIC: public members have Public visibility
/// - Q-ITEM-MODULE: class qualified under module
/// - R-CONTAINS-CLASS-MEMBER: Class CONTAINS methods
pub static CLASSES: Fixture = Fixture {
    name: "ts_classes",
    files: &[(
        "user.ts",
        r#"
export class User {
    public name: string;
    private id: number;

    constructor(name: string, id: number) {
        this.name = name;
        this.id = id;
    }

    public greet(): string {
        return `Hello, ${this.name}!`;
    }

    private validate(): boolean {
        return this.id > 0;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "user.User",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "user.User.id",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "user.User.constructor",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "user.User.greet",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "user.User.validate",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "user.User",
            to: "user.User.name",
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

/// Abstract classes
///
/// Validates:
/// - E-CLASS-ABSTRACT: abstract class produces Class entity
/// - E-METHOD-ABSTRACT: abstract methods produce Method entities
/// - M-CLASS-ABSTRACT: abstract classes have is_abstract metadata
pub static ABSTRACT_CLASSES: Fixture = Fixture {
    name: "ts_abstract_classes",
    files: &[(
        "shape.ts",
        r#"
export abstract class Shape {
    abstract area(): number;
    abstract perimeter(): number;

    describe(): string {
        return `Area: ${this.area()}, Perimeter: ${this.perimeter()}`;
    }
}

export class Circle extends Shape {
    constructor(public radius: number) {
        super();
    }

    area(): number {
        return Math.PI * this.radius ** 2;
    }

    perimeter(): number {
        return 2 * Math.PI * this.radius;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "shape.Shape",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "shape.Shape.area",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "shape.Shape.perimeter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "shape.Shape.describe",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "shape.Circle",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::InheritsFrom,
            from: "shape.Circle",
            to: "shape.Shape",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Class expressions
///
/// Validates:
/// - E-CLASS-EXPR: class expression assigned to variable produces Class entity
pub static CLASS_EXPRESSIONS: Fixture = Fixture {
    name: "ts_class_expressions",
    files: &[(
        "factories.ts",
        r#"
// Named class expression
export const NamedClass = class MyClass {
    value: number = 0;
};

// Anonymous class expression
export const AnonymousClass = class {
    data: string = "";
};
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "factories.MyClass",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "factories.AnonymousClass",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Class inheritance
///
/// Validates:
/// - E-CLASS: both base and derived classes produce Class entities
/// - R-INHERITS-FROM: derived class INHERITS_FROM base class
pub static CLASS_INHERITANCE: Fixture = Fixture {
    name: "ts_class_inheritance",
    files: &[(
        "animals.ts",
        r#"
export class Animal {
    constructor(public name: string) {}

    speak(): void {
        console.log(`${this.name} makes a sound`);
    }
}

export class Dog extends Animal {
    constructor(name: string, public breed: string) {
        super(name);
    }

    speak(): void {
        console.log(`${this.name} barks`);
    }

    fetch(): void {
        console.log(`${this.name} fetches the ball`);
    }
}

export class GermanShepherd extends Dog {
    constructor(name: string) {
        super(name, "German Shepherd");
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "animals.Animal",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "animals.Dog",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "animals.GermanShepherd",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::InheritsFrom,
            from: "animals.Dog",
            to: "animals.Animal",
        },
        ExpectedRelationship {
            kind: RelationshipKind::InheritsFrom,
            from: "animals.GermanShepherd",
            to: "animals.Dog",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Class implementing interfaces
///
/// Validates:
/// - E-CLASS: class produces Class entity
/// - E-INTERFACE: interface produces Interface entity
/// - R-IMPLEMENTS: class IMPLEMENTS interface
pub static CLASS_IMPLEMENTS: Fixture = Fixture {
    name: "ts_class_implements",
    files: &[(
        "disposable.ts",
        r#"
export interface Disposable {
    dispose(): void;
}

export interface Serializable {
    serialize(): string;
    deserialize(data: string): void;
}

export class Resource implements Disposable, Serializable {
    private data: string = "";

    dispose(): void {
        this.data = "";
    }

    serialize(): string {
        return this.data;
    }

    deserialize(data: string): void {
        this.data = data;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "disposable.Disposable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Interface,
            qualified_name: "disposable.Serializable",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "disposable.Resource",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "disposable.Resource",
            to: "disposable.Disposable",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "disposable.Resource",
            to: "disposable.Serializable",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Class fields with various modifiers
///
/// Validates:
/// - E-PROPERTY-FIELD: field declarations produce Property entities
/// - Q-PROPERTY-INSTANCE: instance fields use Class::field format
/// - M-FIELD-INITIALIZER: fields with initializers tracked
pub static CLASS_FIELDS: Fixture = Fixture {
    name: "ts_class_fields",
    files: &[(
        "config.ts",
        r#"
export class Config {
    // Field with initializer
    public enabled: boolean = true;

    // Field without initializer
    public name: string;

    // Readonly field
    public readonly version: string = "1.0.0";

    // Optional field
    public description?: string;

    constructor(name: string) {
        this.name = name;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "config.Config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.enabled",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.version",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "config.Config.description",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "config.Config",
            to: "config.Config.enabled",
        },
    ],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Constructor parameter properties
///
/// Validates:
/// - E-PROPERTY-PARAM: constructor parameters with visibility modifiers produce Property entities
pub static PARAMETER_PROPERTIES: Fixture = Fixture {
    name: "ts_parameter_properties",
    files: &[(
        "point.ts",
        r#"
export class Point {
    constructor(
        public x: number,
        public y: number,
        private _label?: string,
        protected readonly id: number = 0
    ) {}

    get label(): string {
        return this._label ?? "unnamed";
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "point.Point",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "point.Point.x",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "point.Point.y",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "point.Point._label",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "point.Point.id",
            visibility: Some(Visibility::Protected),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Private class fields (ES2022 #private syntax)
///
/// Validates:
/// - E-PROPERTY-PRIVATE-FIELD: #private fields produce Property entities
/// - V-CLASS-PRIVATE-FIELD: #private fields have Private visibility
pub static PRIVATE_FIELDS: Fixture = Fixture {
    name: "ts_private_fields",
    files: &[(
        "counter.ts",
        r#"
export class Counter {
    #count: number = 0;
    #maxValue: number;

    constructor(maxValue: number = 100) {
        this.#maxValue = maxValue;
    }

    increment(): void {
        if (this.#count < this.#maxValue) {
            this.#count++;
        }
    }

    get value(): number {
        return this.#count;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "counter.Counter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "counter.Counter.#count",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "counter.Counter.#maxValue",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "counter.Counter.increment",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Static class members
///
/// Validates:
/// - E-METHOD-STATIC: static methods produce Method entities
/// - E-PROPERTY-STATIC: static properties produce Property entities
/// - Q-METHOD-STATIC: static methods use Class.method format
/// - Q-PROPERTY-STATIC: static properties use Class.property format
/// - M-METHOD-STATIC: static methods have is_static metadata
pub static STATIC_MEMBERS: Fixture = Fixture {
    name: "ts_static_members",
    files: &[(
        "math.ts",
        r#"
export class MathUtils {
    static readonly PI: number = 3.14159;
    static E: number = 2.71828;

    static add(a: number, b: number): number {
        return a + b;
    }

    static multiply(a: number, b: number): number {
        return a * b;
    }

    // Instance method for comparison
    instanceMethod(): void {}
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "math.MathUtils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "math.MathUtils.PI",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "math.MathUtils.E",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "math.MathUtils.add",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "math.MathUtils.multiply",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "math.MathUtils.instanceMethod",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};

/// Getter and setter accessors
///
/// Validates:
/// - E-METHOD-GETTER: getter produces Method entity
/// - E-METHOD-SETTER: setter produces Method entity
/// - M-METHOD-ACCESSOR-GET: getter has accessor type metadata
/// - M-METHOD-ACCESSOR-SET: setter has accessor type metadata
pub static ACCESSORS: Fixture = Fixture {
    name: "ts_accessors",
    files: &[(
        "temperature.ts",
        r#"
export class Temperature {
    private _celsius: number = 0;

    get celsius(): number {
        return this._celsius;
    }

    set celsius(value: number) {
        this._celsius = value;
    }

    get fahrenheit(): number {
        return (this._celsius * 9) / 5 + 32;
    }

    set fahrenheit(value: number) {
        this._celsius = ((value - 32) * 5) / 9;
    }

    // Read-only accessor (getter only)
    get kelvin(): number {
        return this._celsius + 273.15;
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Class,
            qualified_name: "temperature.Temperature",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "temperature.Temperature._celsius",
            visibility: Some(Visibility::Private),
        },
        // Getters
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "temperature.Temperature.celsius",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "temperature.Temperature.fahrenheit",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "temperature.Temperature.kelvin",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[],
    project_type: ProjectType::TypeScriptProject,
    manifest: None,
};
