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
/// - `export const foo = function() {}`
/// - `(function foo() {})()` (IIFE)
///
/// For named function expressions like `const x = function bar() {}`, we capture
/// the function's own name (`bar`) as `@fn_name`, falling back to the variable
/// name (`x`) as `@name` if the function is anonymous.
///
/// This query matches at the variable_declarator level to avoid duplicate matches
/// for exported vs non-exported patterns.
pub(crate) const FUNCTION_EXPRESSION_QUERY: &str = r#"
;; Match at variable_declarator level - works for both exported and non-exported
(variable_declarator
  name: (identifier) @name
  value: [
    (function_expression
      name: (identifier)? @fn_name
      parameters: (formal_parameters) @params
      body: (statement_block) @body)
    (generator_function
      name: (identifier)? @fn_name
      parameters: (formal_parameters) @params
      body: (statement_block) @body)
  ] @value) @function

;; IIFE (Immediately Invoked Function Expression): (function name() {})()
;; The function expression is inside a parenthesized_expression which is called
(call_expression
  function: (parenthesized_expression
    (function_expression
      name: (identifier) @fn_name
      parameters: (formal_parameters) @params
      body: (statement_block) @body) @value)) @function
"#;

/// Query for arrow functions assigned to variables
///
/// Matches:
/// - `const foo = () => {}`
/// - `const foo = (x) => x * 2`
/// - `const foo = async () => {}`
/// - `export const foo = () => {}`
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

(export_statement
  declaration: (lexical_declaration
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
        ] @body)))) @function
"#;
