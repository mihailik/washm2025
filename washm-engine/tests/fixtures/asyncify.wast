(module
  (import "env" "imp" (func $imp))
  (func $f (export "f")
    (call $imp)
  )
)
