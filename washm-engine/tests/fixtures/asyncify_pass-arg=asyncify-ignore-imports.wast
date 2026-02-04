(module
  (import "env" "imp" (func $imp (param i32)))
  (func $h (export "h") (param $x i32)
    (call $imp (local.get $x))
  )
)
