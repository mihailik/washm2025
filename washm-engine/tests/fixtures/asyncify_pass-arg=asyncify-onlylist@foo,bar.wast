(module
  (import "env" "foo" (func $foo))
  (import "env" "bar" (func $bar))
  (func $a (export "a")
    (call $foo)
    (call $bar)
  )
)
