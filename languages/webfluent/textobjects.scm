; Vim-mode text objects and motions.
;
; A "function" is anything with a body that runs: an action, an effect, an
; event handler, a middleware, a lambda. A "class" is a top-level
; declaration. The inside of a block is its statements — its named children
; — so a braced block and an indented one (`.wfx`) are read alike.

(action_declaration
  body: (block
    (_)* @function.inside)) @function.around

(effect_declaration
  body: (block
    (_)* @function.inside)) @function.around

(event_handler
  body: (block
    (_)* @function.inside)) @function.around

(middleware_declaration
  body: (block
    (_)* @function.inside)) @function.around

(fake_answer
  value: (_) @function.inside) @function.around

(lambda
  body: (_) @function.inside) @function.around

(page_declaration
  body: (block
    (_)* @class.inside)) @class.around

(component_declaration
  body: (block
    (_)* @class.inside)) @class.around

(store_declaration
  body: (block
    (_)* @class.inside)) @class.around

(app_declaration
  body: (block
    (_)* @class.inside)) @class.around

(test_declaration
  body: (block
    (_)* @class.inside)) @class.around

(example_declaration
  body: (block
    (_)* @class.inside)) @class.around

(fake_declaration
  body: (fake_body
    (_)* @class.inside)) @class.around

(api_declaration
  body: (api_body
    (_)* @class.inside)) @class.around

(theme_declaration
  body: (theme_body
    (_)* @class.inside)) @class.around

(type_declaration
  (field_declaration)+ @class.inside) @class.around

(enum_declaration
  (enum_case_declaration)+ @class.inside) @class.around

(comment)+ @comment.around
