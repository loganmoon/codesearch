; Sample .scm query file for Rust entity extraction (V2 Architecture)
;
; This file demonstrates the format for handler-annotated queries.
; Each query is preceded by a "; @handler" comment specifying the handler name.

; @handler rust::free_function
; Free functions at module level (not inside impl blocks)
((function_item
  name: (identifier) @name
) @func
(#not-has-ancestor? @func impl_item))

; @handler rust::inherent_impl
; Inherent impl blocks (no trait)
((impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list) @body
) @impl
(#not-has-child? @impl trait))

; @handler rust::method_in_inherent_impl
; Methods inside inherent impl blocks
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

; @handler rust::struct_definition
; Struct definitions
(struct_item
  name: (type_identifier) @name
) @struct
