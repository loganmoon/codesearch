; Rust Entity Extraction Queries (V2 Architecture)
;
; This file defines tree-sitter queries for extracting Rust entities.
; Each query is annotated with handler metadata:
;   @handler     - Handler name (language::handler_name)
;   @entity_type - Entity type produced (Function, Method, Struct, etc.)
;   @capture     - Primary capture name for the entity
;   @description - Optional description

; =============================================================================
; Functions
; =============================================================================

; @handler rust::free_function
; @entity_type Function
; @capture func
; @description Free functions at module level (not inside impl blocks)
((function_item
  name: (identifier) @name
) @func
(#not-has-ancestor? @func impl_item))

; =============================================================================
; Impl Blocks
; =============================================================================

; @handler rust::inherent_impl
; @entity_type Impl
; @capture impl
; @description Inherent impl blocks (no trait)
((impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list) @body
) @impl
(#not-has-child? @impl trait))

; @handler rust::trait_impl
; @entity_type Impl
; @capture impl
; @description Trait impl blocks (impl Trait for Type)
(impl_item
  trait: (type_identifier) @trait_name
  type: (type_identifier) @impl_type
  body: (declaration_list) @body
) @impl

; =============================================================================
; Methods
; =============================================================================

; @handler rust::method_in_inherent_impl
; @entity_type Method
; @capture method
; @description Methods with self parameter in inherent impl blocks
((impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list
    (function_item
      name: (identifier) @name
      parameters: (parameters
        . (self_parameter) @self_param
      )
    ) @method
  )
) @impl
(#not-has-child? @impl trait))

; @handler rust::method_in_trait_impl
; @entity_type Method
; @capture method
; @description Methods in trait impl blocks
(impl_item
  trait: (type_identifier) @trait_name
  type: (type_identifier) @impl_type
  body: (declaration_list
    (function_item
      name: (identifier) @name
      parameters: (parameters) @params
    ) @method
  )
) @impl

; =============================================================================
; Type Definitions
; =============================================================================

; @handler rust::struct_definition
; @entity_type Struct
; @capture struct
; @description Struct definitions
(struct_item
  name: (type_identifier) @name
) @struct

; @handler rust::enum_definition
; @entity_type Enum
; @capture enum
; @description Enum definitions
(enum_item
  name: (type_identifier) @name
) @enum

; @handler rust::trait_definition
; @entity_type Trait
; @capture trait
; @description Trait definitions
(trait_item
  name: (type_identifier) @name
) @trait
