//! Function-related queries for JavaScript and TypeScript

/// Query for function declarations
///
/// Matches:
/// - `function foo() {}`
/// - `async function foo() {}`
/// - `function* foo() {}`
/// - `async function* foo() {}`
/// - `export function foo() {}`
pub(crate) const FUNCTION_DECLARATION_QUERY: &str = r#"
[
  (function_declaration
    name: (identifier) @name
    parameters: (formal_parameters) @params
    body: (statement_block) @body) @function

  (generator_function_declaration
    name: (identifier) @name
    parameters: (formal_parameters) @params
    body: (statement_block) @body) @function

  (export_statement
    declaration: (function_declaration
      name: (identifier) @name
      parameters: (formal_parameters) @params
      body: (statement_block) @body)) @function

  (export_statement
    declaration: (generator_function_declaration
      name: (identifier) @name
      parameters: (formal_parameters) @params
      body: (statement_block) @body)) @function
]
"#;

/// Query for function expressions assigned to variables
///
/// Matches:
/// - `const foo = function() {}`
/// - `const foo = function bar() {}`
/// - `let foo = function() {}`
/// - `var foo = function() {}`
pub(crate) const FUNCTION_EXPRESSION_QUERY: &str = r#"
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: [
      (function_expression
        parameters: (formal_parameters) @params
        body: (statement_block) @body)
      (generator_function
        parameters: (formal_parameters) @params
        body: (statement_block) @body)
    ] @value)) @function

(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: [
      (function_expression
        parameters: (formal_parameters) @params
        body: (statement_block) @body)
      (generator_function
        parameters: (formal_parameters) @params
        body: (statement_block) @body)
    ] @value)) @function
"#;

/// Query for arrow functions assigned to variables
///
/// Matches:
/// - `const foo = () => {}`
/// - `const foo = (x) => x * 2`
/// - `const foo = async () => {}`
pub(crate) const ARROW_FUNCTION_QUERY: &str = r#"
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (arrow_function
      parameters: [
        (formal_parameters) @params
        (identifier) @params
      ]?
      body: [
        (statement_block)
        (_)
      ] @body))) @function

(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: (arrow_function
      parameters: [
        (formal_parameters) @params
        (identifier) @params
      ]?
      body: [
        (statement_block)
        (_)
      ] @body))) @function
"#;

/// Query for default exported functions
///
/// Matches:
/// - `export default function() {}`
/// - `export default function foo() {}`
/// - `export default async function() {}`
pub(crate) const _DEFAULT_EXPORT_FUNCTION_QUERY: &str = r#"
(export_statement
  (function_declaration
    name: (identifier)? @name
    parameters: (formal_parameters) @params
    body: (statement_block) @body) @value
  "default" @default) @function

(export_statement
  (generator_function_declaration
    name: (identifier)? @name
    parameters: (formal_parameters) @params
    body: (statement_block) @body) @value
  "default" @default) @function
"#;
