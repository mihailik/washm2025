(module
  (import "env" "imp" (func $imp))
  (func $g (export "g")
    (call $imp)
  )
)
